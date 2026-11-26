//! Boîte privée (lot F, ADR 013) : la seule boîte `BoxKind::Private`, adossée au
//! coffre chiffré de `tidy-vault`.
//!
//! - **Verrouillée** : écran de verrouillage dans la boîte (mot de passe, « Mot de
//!   passe oublié ? »), opacité réglable jusqu'à 0 % avec retour au survol.
//! - **Déverrouillée** : le contenu du coffre, dessiné comme une boîte ordinaire.
//!   Ouvrir déchiffre dans le dossier temporaire du coffre ; « Remettre sur le
//!   bureau » extrait ; glisser un élément **du bureau** dedans l'ajoute au coffre
//!   après confirmation (l'original part ensuite à la corbeille).
//! - Création, mot de passe oublié, changement de mot de passe et suppression (qui rend
//!   tous les fichiers au bureau) passent par une petite fenêtre à part.
//! - Le coffre vit sur un **fil à part** (Argon2id, chiffrement des gros fichiers) ; il
//!   se verrouille seul après N minutes sans clavier ni souris, à la mise en veille et
//!   au verrouillage de la session, et avant la fin du processus.
//!
//! Mots de passe et clés ne quittent jamais ce processus, ne sont jamais journalisés
//! (ni les noms des fichiers du coffre) et sont effacés de la mémoire après usage.
//!
//! Branchement dans le bureau (`mod.rs`) : quelques appels seulement ; la logique est
//! ici, dans un `impl Desktop`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use egui::{
    Align, Color32, CornerRadius, FontId, Key, Layout, Modifiers, RichText, Stroke, StrokeKind,
    TextEdit, Ui, ViewportBuilder, ViewportCommand, ViewportId, WindowLevel, vec2,
};
use tidy_core::category::classify_by_extension;
use tidy_core::geometry::{Point, Rect};
use tidy_core::model::{BoxId, BoxKind, DeskBox};
use tidy_core::rules::ItemInfo;
use tidy_core::settings::vault::VaultSettings;
use tidy_core::{DesktopState, Store};
use tidy_platform::SessionEvent;
use tidy_ui::{icons, t, theme, widgets};
use tidy_vault::{
    FileVault, RecoveryKey, Secret, VaultBackend, VaultError, VaultItem, Zeroizing, check_password,
};

use super::Desktop;
use super::action::Action;
use super::boxwin::{self, BoxOutput, BoxUi, BoxView};
use super::items::{self, ItemDir};
use super::menu::{Entry, EntryKind, Page};
use super::overlay::{self, PopupKind};
use super::textures::IconCache;

/// Nom du module dans `Request::Module` / `Event::Module`.
pub const MODULE: &str = "vault";
/// Fréquence de la vérification de l'inactivité (fil du coffre).
const IDLE_POLL: Duration = Duration::from_secs(5);
/// Au plus un signal d'activité par intervalle (survol de la boîte).
const TOUCH_EVERY: Duration = Duration::from_secs(10);
/// Attente maximale du verrouillage avant la fin du processus.
const EXIT_WAIT: Duration = Duration::from_secs(10);
const DIALOG_WIDTH: f32 = 380.0;

/// Coffre déverrouillé (lu par le menu ☰).
static UNLOCKED: AtomicBool = AtomicBool::new(false);
/// Coffre existant (lu par le menu ☰).
static EXISTS: AtomicBool = AtomicBool::new(false);
/// File du fil du coffre, pour verrouiller avant la fin du processus.
static EXIT: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();

/// Demandes propres à la boîte privée (variante [`Action::Vault`]).
#[derive(Debug, Clone, PartialEq)]
pub enum VaultAction {
    /// « Boîte privée » (menu ☰) : la boîte si le coffre existe, sinon l'assistant.
    NewPrivateBox {
        near: Option<BoxId>,
    },
    /// Assistant de création.
    Create,
    /// « Mot de passe oublié ? ».
    Forgot,
    Lock,
    ChangePassword,
    /// « Supprimer la boîte privée » (mot de passe demandé).
    AskDelete,
    /// Ajouter au coffre (après confirmation) des éléments du bureau.
    Add {
        paths: Vec<PathBuf>,
    },
    /// « Remettre sur le bureau ».
    Extract {
        names: Vec<String>,
    },
    Open {
        names: Vec<String>,
    },
    /// Mettre à la corbeille (après confirmation).
    Trash {
        names: Vec<String>,
    },
}

// ---------------------------------------------------------------------------
// Fil du coffre
// ---------------------------------------------------------------------------

enum Job {
    Settings(VaultSettings),
    Session(SessionEvent),
    /// L'utilisateur se sert de la boîte (inactivité, quand le système ne la donne pas).
    Touch,
    Create {
        password: Secret,
        recovery: RecoveryKey,
    },
    Unlock {
        password: Secret,
    },
    Recover {
        recovery: Secret,
        password: Secret,
    },
    Lock,
    /// Verrouiller, puis prévenir (fin du processus).
    LockAndAck(SyncSender<()>),
    Add {
        paths: Vec<PathBuf>,
    },
    Extract {
        names: Vec<String>,
        dest: PathBuf,
    },
    Open {
        names: Vec<String>,
    },
    Trash {
        names: Vec<String>,
    },
    Rename {
        old: String,
        new: String,
    },
    ChangePassword {
        old: Secret,
        new: Secret,
    },
    Destroy {
        password: Secret,
        dest: PathBuf,
    },
}

/// Ce que fait (ou vient de faire) le fil du coffre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Task {
    Create,
    Unlock,
    Recover,
    Lock,
    AutoLock,
    Add,
    Extract,
    Open,
    Trash,
    Rename,
    ChangePassword,
    Destroy,
}

impl Task {
    fn label(self) -> &'static str {
        match self {
            Task::Create => t!("vault.busy_create"),
            Task::Unlock | Task::Recover => t!("vault.busy_unlock"),
            Task::Lock | Task::AutoLock => t!("vault.busy_lock"),
            Task::Add => t!("vault.busy_add"),
            Task::Extract | Task::Destroy => t!("vault.busy_extract"),
            Task::Open => t!("vault.busy_open"),
            Task::Trash | Task::Rename | Task::ChangePassword => t!("vault.busy_other"),
        }
    }
}

/// Réponse du fil, avec l'état du coffre après coup.
struct Reply {
    task: Task,
    /// Erreur (texte pour l'utilisateur), la première rencontrée.
    error: Option<String>,
    /// Mot de passe ou clé de récupération faux.
    wrong_secret: bool,
    exists: bool,
    unlocked: bool,
    items: Vec<VaultItem>,
    /// Ajout : originaux partis à la corbeille.
    moved: Vec<PathBuf>,
    /// Ouverture : copies à ouvrir.
    opened: Vec<PathBuf>,
    /// Éléments traités.
    count: usize,
    /// Copies laissées en clair au verrouillage.
    kept: usize,
}

impl Reply {
    fn new(task: Task) -> Self {
        Self {
            task,
            error: None,
            wrong_secret: false,
            exists: false,
            unlocked: false,
            items: Vec::new(),
            moved: Vec::new(),
            opened: Vec::new(),
            count: 0,
            kept: 0,
        }
    }

    fn fail(&mut self, e: &VaultError) {
        // Jamais de secret ni de nom de fichier dans le journal : le genre d'erreur
        // seulement.
        log::warn!("coffre ({:?}) : {}", self.task, error_kind(e));
        if matches!(e, VaultError::WrongPassword | VaultError::WrongRecoveryKey) {
            self.wrong_secret = true;
        }
        if self.error.is_none() {
            self.error = Some(error_text(e));
        }
    }
}

/// Genre d'erreur, pour le journal.
fn error_kind(e: &VaultError) -> &'static str {
    match e {
        VaultError::NotCreated => "pas de coffre",
        VaultError::AlreadyExists => "coffre déjà créé",
        VaultError::Locked => "verrouillé",
        VaultError::WrongPassword => "mot de passe incorrect",
        VaultError::WrongRecoveryKey => "clé de récupération incorrecte",
        VaultError::BadPassword => "longueur du mot de passe",
        VaultError::BadRecoveryKey => "clé mal formée",
        VaultError::Corrupt(_) => "coffre illisible",
        VaultError::Verification(_) => "vérification échouée",
        VaultError::NotFound(_) => "élément introuvable",
        VaultError::BadName(_) => "nom refusé",
        VaultError::NameTaken(_) => "nom déjà pris",
        VaultError::Unsupported(_) => "non pris en charge",
        VaultError::Symlink => "lien symbolique",
        VaultError::StillOpen => "fichiers encore ouverts",
        VaultError::Disposal(_) => "corbeille refusée",
        VaultError::Random(_) => "aléa indisponible",
        VaultError::Io { context, .. } => context,
    }
}

/// Texte d'une erreur pour l'utilisateur.
fn error_text(e: &VaultError) -> String {
    match e {
        VaultError::WrongPassword => t!("vault.err_wrong_password").into(),
        VaultError::WrongRecoveryKey => t!("vault.err_wrong_recovery").into(),
        VaultError::BadPassword => t!("vault.err_password_length").into(),
        VaultError::BadRecoveryKey => t!("vault.err_recovery_format").into(),
        VaultError::Locked => t!("vault.err_locked").into(),
        VaultError::NotCreated => t!("vault.err_not_created").into(),
        VaultError::AlreadyExists => t!("vault.err_exists").into(),
        VaultError::Disposal(_) => t!("vault.err_trash").into(),
        VaultError::Verification(_) => t!("vault.err_verify").into(),
        VaultError::Corrupt(_) => t!("vault.err_corrupt").into(),
        VaultError::NameTaken(name) => t!("box.rename_exists", name = name),
        VaultError::BadName(_) => t!("box.rename_invalid").into(),
        VaultError::Symlink => t!("vault.err_symlink").into(),
        VaultError::StillOpen => t!("vault.err_still_open").into(),
        e => t!("vault.err_io", detail = e.to_string()),
    }
}

/// Mise à la corbeille par le système (jamais de suppression définitive).
fn to_trash(path: &Path) -> std::io::Result<()> {
    tidy_platform::current()
        .trash(&[path.to_path_buf()])
        .map_err(|e| std::io::Error::other(format!("{e:#}")))
}

/// Boucle du fil du coffre.
fn run(mut vault: FileVault, jobs: Receiver<Job>, replies: Sender<Reply>, wake: egui::Context) {
    let platform = tidy_platform::current();
    let mut settings = VaultSettings::default();
    let mut active = Instant::now();
    loop {
        let job = match jobs.recv_timeout(IDLE_POLL) {
            Ok(job) => job,
            Err(RecvTimeoutError::Timeout) => {
                if vault.is_unlocked() && settings.auto_lock {
                    let idle = platform
                        .idle_seconds()
                        .unwrap_or_else(|| active.elapsed().as_secs_f64());
                    if idle >= settings.idle_limit_secs() {
                        log::info!("coffre : verrouillé après inactivité");
                        let job = Job::Lock;
                        let reply = handle(&mut vault, job, Task::AutoLock);
                        if replies.send(reply).is_err() {
                            break;
                        }
                        wake.request_repaint();
                    }
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let task = match &job {
            Job::Settings(s) => {
                settings = s.clone();
                continue;
            }
            Job::Touch => {
                active = Instant::now();
                continue;
            }
            Job::LockAndAck(ack) => {
                vault.lock();
                let _ = ack.send(());
                continue;
            }
            Job::Session(event) => {
                let wanted = match event {
                    SessionEvent::Sleep => settings.lock_on_sleep,
                    SessionEvent::Locked => settings.lock_on_session_lock,
                    SessionEvent::Unlocked | SessionEvent::Wake => false,
                };
                if !(wanted && vault.is_unlocked()) {
                    continue;
                }
                log::info!("coffre : verrouillé ({event:?})");
                Task::AutoLock
            }
            Job::Create { .. } => Task::Create,
            Job::Unlock { .. } => Task::Unlock,
            Job::Recover { .. } => Task::Recover,
            Job::Lock => Task::Lock,
            Job::Add { .. } => Task::Add,
            Job::Extract { .. } => Task::Extract,
            Job::Open { .. } => Task::Open,
            Job::Trash { .. } => Task::Trash,
            Job::Rename { .. } => Task::Rename,
            Job::ChangePassword { .. } => Task::ChangePassword,
            Job::Destroy { .. } => Task::Destroy,
        };
        active = Instant::now();
        let reply = handle(&mut vault, job, task);
        if replies.send(reply).is_err() {
            break;
        }
        wake.request_repaint();
    }
    // Le bureau s'arrête : rien ne doit rester déverrouillé.
    vault.lock();
}

/// Exécute une demande et rend la réponse (avec l'état du coffre).
fn handle(vault: &mut FileVault, job: Job, task: Task) -> Reply {
    let mut r = Reply::new(task);
    let mut dispose = |p: &Path| to_trash(p);
    match job {
        Job::Create { password, recovery } => {
            if let Err(e) = vault.create(&password, &recovery) {
                r.fail(&e);
            }
        }
        Job::Unlock { password } => {
            if let Err(e) = vault.unlock(&password) {
                r.fail(&e);
            }
        }
        Job::Recover { recovery, password } => {
            if let Err(e) = vault.recover(&recovery, &password) {
                r.fail(&e);
            }
        }
        Job::Lock | Job::Session(_) => {
            let report = vault.lock();
            r.count = report.updated;
            r.kept = report.kept.len();
        }
        Job::Add { paths } => {
            for path in paths {
                match vault.add(&path, &mut dispose) {
                    Ok(_) => r.moved.push(path),
                    Err(e) => r.fail(&e),
                }
            }
            r.count = r.moved.len();
        }
        Job::Extract { names, dest } => {
            for name in names {
                match vault.extract(&name, &dest) {
                    Ok(_) => r.count += 1,
                    Err(e) => r.fail(&e),
                }
            }
        }
        Job::Open { names } => {
            for name in names {
                match vault.open(&name) {
                    Ok(p) => r.opened.push(p),
                    Err(e) => r.fail(&e),
                }
            }
        }
        Job::Trash { names } => {
            for name in names {
                match vault.trash(&name, &mut dispose) {
                    Ok(()) => r.count += 1,
                    Err(e) => r.fail(&e),
                }
            }
        }
        Job::Rename { old, new } => {
            if let Err(e) = vault.rename(&old, &new) {
                r.fail(&e);
            }
        }
        Job::ChangePassword { old, new } => {
            if let Err(e) = vault.change_password(&old, &new) {
                r.fail(&e);
            }
        }
        Job::Destroy { password, dest } => match vault.destroy(&password, &dest) {
            Ok(returned) => r.count = returned.len(),
            Err(e) => r.fail(&e),
        },
        Job::Settings(_) | Job::Touch | Job::LockAndAck(_) => {}
    }
    r.exists = vault.exists();
    r.unlocked = vault.is_unlocked();
    r.items = vault.list().unwrap_or_default();
    r
}

/// Verrouille le coffre avant la fin du processus (agent parti, `Shutdown`) : les
/// fichiers ouverts modifiés sont rechiffrés et leurs copies effacées. Attend au plus
/// quelques secondes.
pub fn lock_before_exit() {
    let Some(tx) = EXIT.get() else {
        return;
    };
    let (ack_tx, ack) = mpsc::sync_channel(1);
    let sent = tx
        .lock()
        .map(|tx| tx.send(Job::LockAndAck(ack_tx)).is_ok())
        .unwrap_or(false);
    if sent {
        let _ = ack.recv_timeout(EXIT_WAIT);
    }
}

// ---------------------------------------------------------------------------
// État d'interface
// ---------------------------------------------------------------------------

/// Petite fenêtre des formulaires.
enum Dialog {
    Create {
        near: Option<BoxId>,
        step: usize,
        password: Secret,
        confirm: Secret,
        recovery: RecoveryKey,
        noted: bool,
        error: Option<String>,
    },
    Recover {
        recovery: Secret,
        password: Secret,
        confirm: Secret,
        error: Option<String>,
    },
    ChangePassword {
        old: Secret,
        password: Secret,
        confirm: Secret,
        error: Option<String>,
    },
    Delete {
        password: Secret,
        error: Option<String>,
    },
}

impl Dialog {
    fn error_mut(&mut self) -> &mut Option<String> {
        match self {
            Dialog::Create { error, .. }
            | Dialog::Recover { error, .. }
            | Dialog::ChangePassword { error, .. }
            | Dialog::Delete { error, .. } => error,
        }
    }
}

/// État de la boîte privée dans le processus du bureau.
pub struct VaultBox {
    jobs: Option<Sender<Job>>,
    replies: Receiver<Reply>,
    root: PathBuf,
    exists: bool,
    unlocked: bool,
    busy: Option<Task>,
    items: Vec<VaultItem>,
    /// Mot de passe en cours de saisie sur l'écran de verrouillage.
    password: Secret,
    lock_error: Option<String>,
    dialog: Option<Dialog>,
    dialog_serial: u64,
    dialog_height: f32,
    dialog_focus: bool,
    sent_settings: Option<VaultSettings>,
    touched: Instant,
}

impl VaultBox {
    /// Lance le fil du coffre et la surveillance de la session. Sur le thread
    /// principal, boucle lancée.
    pub fn new(store: &Store, ctx: &egui::Context) -> Self {
        let root = store.root().join("vault");
        let (jobs_tx, jobs_rx) = mpsc::channel();
        let (reply_tx, replies) = mpsc::channel();
        let vault = FileVault::new(&root);
        let wake = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("tidy-coffre".into())
            .spawn(move || run(vault, jobs_rx, reply_tx, wake));
        let jobs = match spawned {
            Ok(_) => Some(jobs_tx),
            Err(e) => {
                log::warn!("fil du coffre : {e}");
                None
            }
        };
        if let Some(tx) = &jobs {
            let _ = EXIT.set(Mutex::new(tx.clone()));
            let session = Mutex::new(tx.clone());
            let watching =
                tidy_platform::current().watch_session(Box::new(move |event: SessionEvent| {
                    if let Ok(tx) = session.lock() {
                        let _ = tx.send(Job::Session(event));
                    }
                }));
            log::info!("surveillance de la session (coffre) : {watching}");
        }
        let exists = FileVault::exists_in(&root);
        EXISTS.store(exists, Ordering::Relaxed);
        Self {
            jobs,
            replies,
            root,
            exists,
            unlocked: false,
            busy: None,
            items: Vec::new(),
            password: Secret::default(),
            lock_error: None,
            dialog: None,
            dialog_serial: 0,
            dialog_height: 200.0,
            dialog_focus: false,
            sent_settings: None,
            touched: Instant::now(),
        }
    }

    /// Envoie une demande au fil. `false` si le fil n'est pas là.
    fn submit(&mut self, job: Job, task: Option<Task>) -> bool {
        let sent = self.jobs.as_ref().is_some_and(|tx| tx.send(job).is_ok());
        if sent {
            if task.is_some() {
                self.busy = task;
            }
        } else {
            self.lock_error = Some(t!("vault.err_worker").into());
        }
        sent
    }

    fn touch(&mut self) {
        if self.unlocked && self.touched.elapsed() >= TOUCH_EVERY {
            self.touched = Instant::now();
            self.submit(Job::Touch, None);
        }
    }

    fn open_dialog(&mut self, dialog: Dialog) {
        self.dialog_serial += 1;
        self.dialog = Some(dialog);
        self.dialog_focus = true;
    }

    /// Contenu affiché d'une boîte : `Some` pour la boîte privée (vide si verrouillée).
    pub fn entries(&self, b: &DeskBox) -> Option<Vec<ItemInfo>> {
        if !matches!(b.kind, BoxKind::Private) {
            return None;
        }
        if !self.unlocked {
            return Some(Vec::new());
        }
        Some(items::dir_entries(
            self.items.iter().map(item_info).collect(),
            b.sort,
        ))
    }

    /// Dossier des éléments de la boîte privée : les icônes y sont cherchées (le
    /// système rend celle du type, ou rien : repli de Tidy).
    pub fn dir(&self) -> ItemDir {
        ItemDir::plain(self.root.join("open.noindex"))
    }

    /// Dessine la boîte privée (à la place de `boxwin::draw`).
    pub fn draw(
        &mut self,
        ui: &mut Ui,
        v: &BoxView,
        st: &mut BoxUi,
        icons: &mut IconCache,
        out: &mut Vec<Action>,
        settings: &VaultSettings,
    ) -> BoxOutput {
        let ctx = ui.ctx().clone();
        let locked = !self.unlocked;
        let hovered = ctx.input(|i| i.pointer.hover_pos().is_some());
        if hovered {
            self.touch();
        }
        // Opacité de la boîte verrouillée, pleine au survol.
        let target = if locked && !hovered && self.dialog.is_none() {
            settings.opacity()
        } else {
            1.0
        };
        let alpha =
            ctx.animate_value_with_time(egui::Id::unique("tidy-coffre-opacité"), target, 0.25);
        // Un voile presque invisible : même à 0 %, la fenêtre reste sensible au survol
        // (macOS laisse passer la souris à travers les pixels transparents).
        ui.painter().rect_filled(
            ui.max_rect(),
            CornerRadius::ZERO,
            Color32::from_black_alpha(2),
        );
        ui.multiply_opacity(alpha);
        let view = BoxView {
            b: v.b,
            look: v.look,
            sections: v.sections,
            dir: v.dir,
            portal: None,
            rolled: v.rolled,
            drop_target: v.drop_target,
            single_click: v.single_click,
            custom_body: locked,
            glass: v.glass,
        };
        let res = boxwin::draw(ui, &view, st, icons, out);
        if let Some(body) = res.body {
            if locked {
                self.draw_lock(ui, body, out);
            } else if let Some(task) = self.busy {
                busy_pill(ui, body, task);
            }
        }
        res
    }

    /// Écran de verrouillage (ou invitation à créer le coffre).
    fn draw_lock(&mut self, ui: &mut Ui, body: egui::Rect, out: &mut Vec<Action>) {
        let inner = body.shrink2(vec2(10.0, 6.0));
        if inner.width() < 40.0 || inner.height() < 20.0 {
            return;
        }
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::top_down(Align::Center)),
        );
        child.set_clip_rect(inner.intersect(ui.clip_rect()));
        let busy = self.busy.is_some();
        egui::ScrollArea::vertical()
            .auto_shrink([false, true])
            .show(&mut child, |ui| {
                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    if !self.exists {
                        ui.label(
                            RichText::new(icons::SHIELD)
                                .size(24.0)
                                .color(theme::ACCENT_2),
                        );
                        ui.add(egui::Label::new(widgets::dim(t!("vault.none"))).wrap());
                        if widgets::primary(ui, t!("vault.create")).clicked() {
                            out.push(Action::Vault(VaultAction::Create));
                        }
                        return;
                    }
                    ui.label(RichText::new(icons::LOCK).size(24.0).color(theme::ACCENT_2));
                    ui.label(RichText::new(t!("vault.locked")).font(theme::strong(theme::SIZE_M)));
                    let width = (inner.width() - 12.0).clamp(60.0, 220.0);
                    let id = egui::Id::unique("tidy-coffre-mot-de-passe");
                    let resp = ui.add_enabled(
                        !busy,
                        TextEdit::singleline(&mut *self.password)
                            .id(id)
                            .password(true)
                            .hint_text(t!("vault.password"))
                            .desired_width(width),
                    );
                    if resp.clicked() {
                        // Une boîte est posée sur le bureau : il faut lui donner le
                        // clavier pour taper.
                        ui.ctx().send_viewport_cmd(ViewportCommand::Focus);
                        resp.request_focus();
                    }
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    let ready = !busy && !self.password.is_empty();
                    let clicked = ui
                        .add_enabled_ui(ready, |ui| widgets::primary(ui, t!("vault.unlock")))
                        .inner
                        .clicked();
                    if ready && (enter || clicked) {
                        let password = Zeroizing::new(std::mem::take(&mut *self.password));
                        forget_field(ui.ctx(), id);
                        self.lock_error = None;
                        self.submit(Job::Unlock { password }, Some(Task::Unlock));
                    }
                    if busy {
                        ui.spinner();
                    }
                    if let Some(e) = &self.lock_error {
                        ui.add(
                            egui::Label::new(
                                RichText::new(e).color(theme::DANGER).size(theme::SIZE_S),
                            )
                            .wrap(),
                        );
                    }
                    if ui
                        .add(
                            egui::Label::new(
                                RichText::new(t!("vault.forgot"))
                                    .size(theme::SIZE_S)
                                    .color(theme::TEXT_DIM)
                                    .underline(),
                            )
                            .sense(egui::Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        out.push(Action::Vault(VaultAction::Forgot));
                    }
                });
            });
    }
}

impl Drop for VaultBox {
    fn drop(&mut self) {
        lock_before_exit();
    }
}

/// Oublie le texte gardé par egui pour un champ (historique d'annulation).
fn forget_field(ctx: &egui::Context, id: egui::Id) {
    ctx.data_mut(|d| d.remove::<egui::text_edit::TextEditState>(id));
}

/// Pastille « Chiffrement… » en bas du contenu.
fn busy_pill(ui: &mut Ui, body: egui::Rect, task: Task) {
    let painter = ui.painter();
    let galley = painter.layout_no_wrap(
        task.label().to_owned(),
        FontId::proportional(theme::SIZE_S),
        theme::TEXT,
    );
    let size = galley.size() + vec2(44.0, 12.0);
    let rect = egui::Rect::from_center_size(
        egui::pos2(body.center().x, body.bottom() - 10.0 - size.y / 2.0),
        size,
    );
    painter.rect_filled(rect, CornerRadius::same(10), theme::SURFACE_HI);
    painter.rect_stroke(
        rect,
        CornerRadius::same(10),
        Stroke::new(1.0, theme::ACCENT),
        StrokeKind::Inside,
    );
    painter.galley(rect.min + vec2(34.0, 6.0), galley, theme::TEXT);
    let spinner =
        egui::Rect::from_center_size(rect.left_center() + vec2(18.0, 0.0), vec2(14.0, 14.0));
    ui.put(spinner, egui::Spinner::new().size(14.0));
}

/// Un élément du coffre, vu comme un élément de boîte.
fn item_info(i: &VaultItem) -> ItemInfo {
    ItemInfo {
        name: i.name.clone(),
        is_dir: i.is_dir,
        category: classify_by_extension(&i.name, i.is_dir),
        size: i.size,
        modified: i
            .modified
            .and_then(|s| jiff::Timestamp::from_second(s).ok()),
        uti: None,
    }
}

/// Nouveau mot de passe valable et confirmé ? Sinon, le texte de l'erreur.
fn check_new(password: &str, confirm: &str) -> Result<(), &'static str> {
    if check_password(password).is_err() {
        Err(t!("vault.err_password_length"))
    } else if password != confirm {
        Err(t!("vault.err_mismatch"))
    } else {
        Ok(())
    }
}

/// Menu des éléments de la boîte privée.
fn item_menu(b: BoxId, names: &[String]) -> Vec<Entry> {
    let names = names.to_vec();
    let entry = |icon, label: &str, kind, danger| Entry::Item {
        icon,
        label: label.to_owned(),
        kind,
        checked: None,
        danger,
    };
    let mut e = vec![entry(
        icons::OPEN,
        t!("box.menu_open"),
        EntryKind::Do(Action::Vault(VaultAction::Open {
            names: names.clone(),
        })),
        false,
    )];
    if let [name] = names.as_slice() {
        e.push(entry(
            icons::EDIT,
            t!("box.menu_rename"),
            EntryKind::Do(Action::StartRename {
                b,
                name: name.clone(),
            }),
            false,
        ));
    }
    e.push(Entry::Separator);
    e.push(entry(
        icons::RESTORE,
        t!("vault.menu_extract"),
        EntryKind::Do(Action::Vault(VaultAction::Extract {
            names: names.clone(),
        })),
        false,
    ));
    e.push(entry(
        icons::DELETE,
        t!("box.menu_trash"),
        EntryKind::Do(Action::AskTrash { b, names }),
        true,
    ));
    e
}

/// Entrées du menu ☰ propres au lot F : « Boîte privée » après « Nouvelle boîte » (s'il
/// n'y en a pas déjà une), et, dans la boîte privée, « Verrouiller » et « Changer le
/// mot de passe ».
pub fn adjust_menu(state: &DesktopState, b: &DeskBox, page: Page, entries: &mut Vec<Entry>) {
    let has_private = state
        .boxes
        .iter()
        .any(|x| matches!(x.kind, BoxKind::Private));
    let new_box = entries.iter().position(|e| {
        matches!(
            e,
            Entry::Item {
                kind: EntryKind::Do(Action::NewBox { .. }),
                ..
            }
        )
    });
    if let Some(i) = new_box
        && !has_private
    {
        entries.insert(
            i + 1,
            Entry::Item {
                icon: icons::LOCK,
                label: t!("vault.menu_private_box").into(),
                kind: EntryKind::Do(Action::Vault(VaultAction::NewPrivateBox {
                    near: Some(b.id),
                })),
                checked: None,
                danger: false,
            },
        );
    }
    if matches!(b.kind, BoxKind::Private) && page == Page::Main && EXISTS.load(Ordering::Relaxed) {
        let mut own = Vec::new();
        if UNLOCKED.load(Ordering::Relaxed) {
            own.push(Entry::Item {
                icon: icons::LOCK,
                label: t!("vault.menu_lock").into(),
                kind: EntryKind::Do(Action::Vault(VaultAction::Lock)),
                checked: None,
                danger: false,
            });
        }
        own.push(Entry::Item {
            icon: icons::KEY,
            label: t!("vault.menu_change_password").into(),
            kind: EntryKind::Do(Action::Vault(VaultAction::ChangePassword)),
            checked: None,
            danger: false,
        });
        own.push(Entry::Separator);
        entries.splice(0..0, own);
    }
}

// ---------------------------------------------------------------------------
// Branchement dans le bureau
// ---------------------------------------------------------------------------

impl Desktop {
    fn private_box(&self) -> Option<BoxId> {
        self.state
            .boxes
            .iter()
            .find(|b| matches!(b.kind, BoxKind::Private))
            .map(|b| b.id)
    }

    /// Dossier des éléments de la boîte `b` si c'est la boîte privée.
    pub(super) fn vault_dir(&self, b: BoxId) -> Option<ItemDir> {
        (self.private_box() == Some(b)).then(|| self.vault.dir())
    }

    /// Dossier des éléments du bureau (bureau de l'utilisateur et bureau public).
    fn desktop_items_dir(&self) -> Option<ItemDir> {
        Some(ItemDir {
            main: self.desktop_dir.clone()?,
            public: self
                .public_dir
                .clone()
                .map(|d| (d, self.public_names.clone())),
        })
    }

    /// Réponses du fil du coffre et réglages. Rend `true` si l'affichage change.
    pub(super) fn vault_poll(&mut self) -> bool {
        let mut changed = false;
        if self.vault.sent_settings.as_ref() != Some(&self.settings.vault) {
            let s = self.settings.vault.clone();
            if self.vault.submit(Job::Settings(s.clone()), None) {
                self.vault.sent_settings = Some(s);
            }
        }
        while let Ok(reply) = self.vault.replies.try_recv() {
            self.vault_reply(reply);
            changed = true;
        }
        // Boîte privée supprimée ailleurs (fenêtre principale) : rien ne reste ouvert.
        if self.vault.unlocked && self.vault.busy.is_none() && self.private_box().is_none() {
            self.vault.submit(Job::Lock, Some(Task::Lock));
        }
        changed
    }

    fn vault_reply(&mut self, r: Reply) {
        if r.task != Task::AutoLock || self.vault.busy == Some(Task::Lock) {
            self.vault.busy = None;
        }
        let was_unlocked = self.vault.unlocked;
        self.vault.exists = r.exists;
        self.vault.unlocked = r.unlocked;
        self.vault.items = r.items;
        EXISTS.store(r.exists, Ordering::Relaxed);
        UNLOCKED.store(r.unlocked, Ordering::Relaxed);
        self.generation += 1;
        let pb = self.private_box();
        let say = |d: &mut Desktop, text: String| {
            if let Some(pb) = pb {
                d.notice(pb, text);
            }
        };
        match r.task {
            Task::Unlock => {
                self.vault.lock_error = r.error;
            }
            Task::Create | Task::Recover | Task::ChangePassword | Task::Destroy => {
                if let Some(e) = r.error {
                    match self.vault.dialog.as_mut() {
                        Some(d) => *d.error_mut() = Some(e),
                        None => say(self, e),
                    }
                    return;
                }
                let dialog = self.vault.dialog.take();
                match r.task {
                    Task::Create => {
                        let near = match dialog {
                            Some(Dialog::Create { near, .. }) => near,
                            _ => None,
                        };
                        let id = self.ensure_private_box(near);
                        self.notice(id, t!("vault.created"));
                    }
                    Task::Recover => {
                        self.vault.lock_error = None;
                        say(self, t!("vault.recovered").into());
                    }
                    Task::ChangePassword => say(self, t!("vault.password_changed").into()),
                    _ => {
                        // Coffre vidé sur le bureau puis supprimé : la boîte part aussi.
                        if let Some(pb) = pb {
                            self.delete_box(pb);
                        }
                        self.overlay
                            .pill_message(t!("vault.destroyed", n = r.count));
                        self.rescan_desktop();
                    }
                }
            }
            Task::Add => {
                if !r.moved.is_empty() {
                    let names: Vec<String> = r
                        .moved
                        .iter()
                        .filter_map(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                        .collect();
                    super::ops::forget_items(&mut self.state, &names);
                    self.save(true);
                    self.rescan_desktop();
                    say(self, t!("vault.added", n = r.count));
                }
                if let Some(e) = r.error {
                    say(self, e);
                }
            }
            Task::Extract => {
                self.rescan_desktop();
                match r.error {
                    Some(e) => say(self, e),
                    None => say(self, t!("vault.extracted", n = r.count)),
                }
            }
            Task::Open => {
                for path in &r.opened {
                    if let Err(e) = tidy_platform::current().open(path) {
                        log::warn!("coffre : ouverture impossible : {e:#}");
                        say(self, t!("vault.open_failed").into());
                    }
                }
                if let Some(e) = r.error {
                    say(self, e);
                }
            }
            Task::Lock | Task::AutoLock => {
                if r.kept > 0 {
                    say(self, t!("vault.kept", n = r.kept));
                } else if was_unlocked && r.task == Task::AutoLock {
                    say(self, t!("vault.auto_locked").into());
                }
                if let Some(id) = pb
                    && let Some(win) = self.windows.get_mut(&id)
                {
                    win.ui.items.selected.clear();
                }
            }
            Task::Trash | Task::Rename => {
                if let Some(e) = r.error {
                    say(self, e);
                }
            }
        }
    }

    /// La boîte privée, créée à côté de `near` (ou sous la souris) si elle manque.
    fn ensure_private_box(&mut self, near: Option<BoxId>) -> BoxId {
        if let Some(id) = self.private_box() {
            return id;
        }
        let mut b = DeskBox::new(t!("vault.box_name"));
        b.kind = BoxKind::Private;
        let rect = match near {
            Some(n) => self.rect_near(n),
            None => {
                let at = tidy_platform::current().mouse_location();
                Rect::new(
                    at.x - super::NEW_BOX_SIZE[0] / 2.0,
                    at.y - boxwin::TITLE_HEIGHT / 2.0,
                    super::NEW_BOX_SIZE[0],
                    super::NEW_BOX_SIZE[1],
                )
            }
        };
        self.hidden = false;
        self.add_box(b, rect)
    }

    /// Fait passer la boîte privée devant quelques secondes.
    fn raise_private_box(&mut self) {
        if let Some(id) = self.private_box() {
            self.hidden = false;
            self.raised_until
                .insert(id, Instant::now() + super::HOTKEY_RAISE);
            self.set_raised(id, true);
            if let Some(win) = self.windows.get(&id) {
                self.ctx
                    .send_viewport_cmd_to(win.viewport, ViewportCommand::Focus);
            }
        }
    }

    /// Demande de la fenêtre principale (`Request::Module { module: "vault" }`).
    pub(super) fn vault_command(&mut self, action: &str) {
        let a = match action {
            "create" => VaultAction::NewPrivateBox { near: None },
            "show" => {
                if self.private_box().is_none() && self.vault.exists {
                    self.ensure_private_box(None);
                }
                self.raise_private_box();
                return;
            }
            "lock" => VaultAction::Lock,
            "change_password" => VaultAction::ChangePassword,
            "forgot" => VaultAction::Forgot,
            "delete" => VaultAction::AskDelete,
            other => {
                log::warn!("coffre : demande inconnue « {other} »");
                return;
            }
        };
        self.vault_action(a);
    }

    /// Applique une [`VaultAction`].
    pub(super) fn vault_action(&mut self, a: VaultAction) {
        match a {
            VaultAction::NewPrivateBox { near } => {
                if self.private_box().is_some() {
                    self.raise_private_box();
                } else if self.vault.exists {
                    self.ensure_private_box(near);
                } else {
                    self.vault_create_dialog(near);
                }
            }
            VaultAction::Create => {
                if !self.vault.exists {
                    self.vault_create_dialog(None);
                }
            }
            VaultAction::Forgot => {
                if self.vault.exists {
                    self.vault.open_dialog(Dialog::Recover {
                        recovery: Secret::default(),
                        password: Secret::default(),
                        confirm: Secret::default(),
                        error: None,
                    });
                }
            }
            VaultAction::Lock => {
                if self.vault.unlocked {
                    self.vault.submit(Job::Lock, Some(Task::Lock));
                }
            }
            VaultAction::ChangePassword => {
                if self.vault.exists {
                    self.vault.open_dialog(Dialog::ChangePassword {
                        old: Secret::default(),
                        password: Secret::default(),
                        confirm: Secret::default(),
                        error: None,
                    });
                }
            }
            VaultAction::AskDelete => self.vault_ask_delete(),
            VaultAction::Add { paths } => {
                if self.vault.unlocked {
                    self.vault.submit(Job::Add { paths }, Some(Task::Add));
                }
            }
            VaultAction::Extract { names } => match self.desktop_dir.clone() {
                Some(dest) => {
                    self.vault
                        .submit(Job::Extract { names, dest }, Some(Task::Extract));
                }
                None => log::warn!("coffre : bureau introuvable"),
            },
            VaultAction::Open { names } => {
                self.vault.submit(Job::Open { names }, Some(Task::Open));
            }
            VaultAction::Trash { names } => {
                self.vault.submit(Job::Trash { names }, Some(Task::Trash));
            }
        }
    }

    fn vault_create_dialog(&mut self, near: Option<BoxId>) {
        match RecoveryKey::generate() {
            Ok(recovery) => self.vault.open_dialog(Dialog::Create {
                near,
                step: 0,
                password: Secret::default(),
                confirm: Secret::default(),
                recovery,
                noted: false,
                error: None,
            }),
            Err(e) => log::warn!("coffre : {}", error_kind(&e)),
        }
    }

    fn vault_ask_delete(&mut self) {
        if self.vault.exists {
            self.vault.open_dialog(Dialog::Delete {
                password: Secret::default(),
                error: None,
            });
        } else if let Some(pb) = self.private_box() {
            // Pas de coffre : rien à rendre, la boîte part simplement.
            self.delete_box(pb);
        }
    }

    /// Demande d'ajouter `paths` (éléments du bureau) au coffre, avec confirmation.
    fn vault_ask_add(&mut self, pb: BoxId, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        if !self.vault.unlocked {
            self.notice(pb, t!("vault.unlock_first"));
            return;
        }
        let title = if paths.len() == 1 {
            t!("vault.add_title_one").to_owned()
        } else {
            t!("vault.add_title", n = paths.len())
        };
        let anchor = self.popup_anchor(pb);
        self.overlay.open(
            PopupKind::Confirm {
                title,
                body: t!("vault.add_body").into(),
                confirm: t!("vault.add_confirm").into(),
                danger: false,
                action: Action::Vault(VaultAction::Add { paths }),
            },
            anchor,
        );
    }

    /// Filtre les actions qui visent la boîte privée : rend celles qu'il faut appliquer
    /// normalement, garde (`None`) celles qu'il traite.
    pub(super) fn vault_intercept(&mut self, action: Action) -> Option<Action> {
        let Some(pb) = self.private_box() else {
            return Some(action);
        };
        // Glisser d'une autre boîte vers la boîte privée.
        if let Action::EndDrag { b, .. } = &action
            && *b != pb
            && self.drag.as_ref().is_some_and(|d| d.from == *b)
            && self.box_under(tidy_platform::current().mouse_location()) == Some(pb)
        {
            let drag = self.drag.take()?;
            if drag.portal {
                self.notice(pb, t!("vault.only_desktop"));
                return None;
            }
            let dir = self.desktop_items_dir()?;
            let paths = drag.names.iter().map(|n| dir.join(n)).collect();
            self.vault_ask_add(pb, paths);
            return None;
        }
        match action {
            Action::Open { b, names } if b == pb => {
                self.vault_action(VaultAction::Open { names });
            }
            Action::ItemMenu { b, names } if b == pb => {
                let entries = item_menu(b, &names);
                self.overlay
                    .open(PopupKind::Menu { entries }, super::mouse_pos());
            }
            Action::Rename { b, old, new } if b == pb => {
                let new = new.trim().to_owned();
                if !new.is_empty() && new != old {
                    self.vault
                        .submit(Job::Rename { old, new }, Some(Task::Rename));
                }
            }
            Action::AskTrash { b, names } | Action::Trash { b, names } if b == pb => {
                let anchor = self.popup_anchor(pb);
                let title = if names.len() == 1 {
                    t!("box.trash_title_one").to_owned()
                } else {
                    t!("box.trash_title", n = names.len())
                };
                self.overlay.open(
                    PopupKind::Confirm {
                        title,
                        body: t!("vault.trash_body").into(),
                        confirm: t!("box.trash_confirm").into(),
                        danger: true,
                        action: Action::Vault(VaultAction::Trash { names }),
                    },
                    anchor,
                );
            }
            Action::RemoveFromBox { b, names } if b == pb => {
                self.vault_action(VaultAction::Extract { names });
            }
            Action::DropFiles { b, paths, .. } if b == pb => {
                let dirs: Vec<PathBuf> = self
                    .desktop_dir
                    .iter()
                    .chain(self.public_dir.iter())
                    .cloned()
                    .collect();
                let (names, outside) = super::split_desktop_items(&paths, &dirs);
                if outside > 0 {
                    self.notice(pb, t!("vault.only_desktop"));
                }
                if let Some(dir) = self.desktop_items_dir() {
                    let paths = names.iter().map(|n| dir.join(n)).collect();
                    self.vault_ask_add(pb, paths);
                }
            }
            Action::StartDrag { b, .. } if b == pb => {
                self.notice(pb, t!("vault.drag_hint"));
            }
            Action::EndDrag { b, .. } | Action::Reveal { b, .. } | Action::CopyPath { b, .. }
                if b == pb => {}
            Action::AskDeleteBox { b } | Action::DeleteBox { b } if b == pb => {
                self.vault_ask_delete();
            }
            other => return Some(other),
        }
        None
    }

    /// Fenêtre des formulaires (création, mot de passe oublié, changement, suppression).
    pub(super) fn vault_dialog(&mut self, ctx: &egui::Context) {
        if self.vault.dialog.is_none() {
            return;
        }
        let anchor = match self.private_box() {
            Some(pb) => self.popup_anchor(pb),
            None => super::mouse_pos(),
        };
        let screen = super::screen_at(
            &self.screens,
            Point {
                x: anchor.x,
                y: anchor.y,
            },
        )
        .visible_frame;
        let size = vec2(DIALOG_WIDTH, self.vault.dialog_height);
        let frame = overlay::place(anchor, size, screen);
        let builder = ViewportBuilder::default()
            .with_title("Tidy · boîte privée")
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_has_shadow(true)
            .with_taskbar(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_active(true)
            .with_position(frame.min)
            .with_inner_size(frame.size());
        let id = ViewportId::from_hash_of(("tidy-coffre", self.vault.dialog_serial));
        let st = &mut self.vault;
        let mut close = false;
        let mut measured = None;
        ctx.show_viewport_immediate(id, builder, |ui, _| {
            let rect = ui.max_rect();
            let radius = CornerRadius::same(12);
            ui.painter().rect_filled(rect, radius, theme::SURFACE);
            ui.painter().rect_stroke(
                rect,
                radius,
                Stroke::new(1.0, theme::BORDER),
                StrokeKind::Inside,
            );
            let inner = rect.shrink(18.0);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
            child.set_width(inner.width());
            close = st.dialog_ui(&mut child);
            measured = Some(child.min_rect().height() + 36.0);
            if st.busy.is_none() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                close = true;
            }
        });
        if let Some(h) = measured
            && (h - self.vault.dialog_height).abs() > 1.0
        {
            self.vault.dialog_height = h;
            let frame = overlay::place(anchor, vec2(DIALOG_WIDTH, h), screen);
            ctx.send_viewport_cmd_to(id, ViewportCommand::InnerSize(frame.size()));
            ctx.send_viewport_cmd_to(id, ViewportCommand::OuterPosition(frame.min));
        }
        if close {
            self.vault.dialog = None;
            ctx.request_repaint();
        }
    }
}

/// Champ de saisie d'un secret, masqué ou non.
fn secret_field(
    ui: &mut Ui,
    text: &mut Secret,
    hint: &str,
    masked: bool,
    focus: bool,
) -> egui::Response {
    let resp = ui.add(
        TextEdit::singleline(&mut **text)
            .password(masked)
            .hint_text(hint)
            .desired_width(f32::INFINITY),
    );
    if focus {
        resp.request_focus();
    }
    resp
}

/// Boutons « Annuler » et action principale, à droite. Rend (annuler, valider).
fn buttons(ui: &mut Ui, label: &str, danger: bool, enabled: bool) -> (bool, bool) {
    let mut out = (false, false);
    ui.add_space(6.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        out.1 = ui
            .add_enabled_ui(enabled, |ui| {
                if danger {
                    widgets::danger(ui, label)
                } else {
                    widgets::primary(ui, label)
                }
            })
            .inner
            .clicked();
        out.0 = ui.button(t!("common.cancel")).clicked();
    });
    out
}

impl VaultBox {
    /// Contenu de la fenêtre des formulaires. Rend `true` pour la fermer.
    fn dialog_ui(&mut self, ui: &mut Ui) -> bool {
        let Some(mut dialog) = self.dialog.take() else {
            return true;
        };
        let busy = self.busy.is_some();
        let focus = std::mem::take(&mut self.dialog_focus);
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        let mut close = false;
        ui.spacing_mut().item_spacing.y = 8.0;
        let title = |ui: &mut Ui, icon: &str, text: &str| {
            ui.label(RichText::new(format!("{icon}  {text}")).font(theme::display(theme::SIZE_L)));
        };
        let error_line = |ui: &mut Ui, error: &Option<String>| {
            if let Some(e) = error {
                ui.add(egui::Label::new(RichText::new(e).color(theme::DANGER)).wrap());
            }
        };
        match &mut dialog {
            Dialog::Create {
                step,
                password,
                confirm,
                recovery,
                noted,
                error,
                ..
            } => {
                title(ui, icons::LOCK, t!("vault.create_title"));
                widgets::step_dots(ui, 2, *step);
                if *step == 0 {
                    ui.add(egui::Label::new(widgets::dim(t!("vault.create_intro"))).wrap());
                    secret_field(ui, password, t!("vault.password"), true, focus);
                    secret_field(ui, confirm, t!("vault.password_confirm"), true, false);
                    error_line(ui, error);
                    let (cancel, next) = buttons(ui, t!("vault.next"), false, true);
                    if next || enter {
                        match check_new(password, confirm) {
                            Ok(()) => {
                                *error = None;
                                *step = 1;
                            }
                            Err(e) => *error = Some(e.to_owned()),
                        }
                    }
                    close = cancel;
                } else {
                    ui.add(egui::Label::new(widgets::dim(t!("vault.recovery_intro"))).wrap());
                    egui::Frame::new()
                        .fill(theme::BG)
                        .stroke(Stroke::new(1.0, theme::ACCENT_2))
                        .corner_radius(10)
                        .inner_margin(12)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new(recovery.display().as_str())
                                        .font(FontId::monospace(18.0))
                                        .color(theme::ACCENT_2),
                                );
                            });
                        });
                    ui.checkbox(noted, t!("vault.recovery_noted"));
                    error_line(ui, error);
                    if busy {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(widgets::dim(Task::Create.label()));
                        });
                    }
                    let (back, create) =
                        buttons(ui, t!("vault.create_confirm"), false, *noted && !busy);
                    if back && !busy {
                        *step = 0;
                    }
                    if create && *noted && !busy {
                        *error = None;
                        let job = Job::Create {
                            password: password.clone(),
                            recovery: recovery.clone(),
                        };
                        self.submit(job, Some(Task::Create));
                    }
                }
            }
            Dialog::Recover {
                recovery,
                password,
                confirm,
                error,
            } => {
                title(ui, icons::KEY, t!("vault.recover_title"));
                ui.add(egui::Label::new(widgets::dim(t!("vault.recover_intro"))).wrap());
                secret_field(ui, recovery, t!("vault.recovery_key"), false, focus);
                secret_field(ui, password, t!("vault.new_password"), true, false);
                secret_field(ui, confirm, t!("vault.password_confirm"), true, false);
                error_line(ui, error);
                if busy {
                    ui.spinner();
                }
                let (cancel, ok) = buttons(ui, t!("vault.recover_confirm"), false, !busy);
                if (ok || enter) && !busy {
                    match check_new(password, confirm) {
                        Ok(()) => {
                            *error = None;
                            let job = Job::Recover {
                                recovery: recovery.clone(),
                                password: password.clone(),
                            };
                            self.submit(job, Some(Task::Recover));
                        }
                        Err(e) => *error = Some(e.to_owned()),
                    }
                }
                close = cancel && !busy;
            }
            Dialog::ChangePassword {
                old,
                password,
                confirm,
                error,
            } => {
                title(ui, icons::KEY, t!("vault.change_title"));
                secret_field(ui, old, t!("vault.old_password"), true, focus);
                secret_field(ui, password, t!("vault.new_password"), true, false);
                secret_field(ui, confirm, t!("vault.password_confirm"), true, false);
                error_line(ui, error);
                if busy {
                    ui.spinner();
                }
                let (cancel, ok) = buttons(ui, t!("vault.change_confirm"), false, !busy);
                if (ok || enter) && !busy {
                    match check_new(password, confirm) {
                        Ok(()) => {
                            *error = None;
                            let job = Job::ChangePassword {
                                old: old.clone(),
                                new: password.clone(),
                            };
                            self.submit(job, Some(Task::ChangePassword));
                        }
                        Err(e) => *error = Some(e.to_owned()),
                    }
                }
                close = cancel && !busy;
            }
            Dialog::Delete { password, error } => {
                title(ui, icons::DELETE, t!("vault.delete_title"));
                ui.add(egui::Label::new(widgets::dim(t!("vault.delete_body"))).wrap());
                secret_field(ui, password, t!("vault.password"), true, focus);
                error_line(ui, error);
                if busy {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(widgets::dim(Task::Destroy.label()));
                    });
                }
                let ready = !busy && !password.is_empty();
                let (cancel, ok) = buttons(ui, t!("vault.delete_confirm"), true, ready);
                if (ok || enter) && ready {
                    *error = None;
                    match dirs_desktop() {
                        Some(dest) => {
                            let job = Job::Destroy {
                                password: password.clone(),
                                dest,
                            };
                            self.submit(job, Some(Task::Destroy));
                        }
                        None => *error = Some(t!("vault.err_no_desktop").into()),
                    }
                }
                close = cancel && !busy;
            }
        }
        if !close {
            self.dialog = Some(dialog);
        }
        close
    }
}

/// Dossier du bureau où rendre les fichiers.
fn dirs_desktop() -> Option<PathBuf> {
    tidy_core::store::desktop_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_passwords_are_checked() {
        assert!(check_new("abcdef", "abcdef").is_ok());
        assert_eq!(
            check_new("abc", "abc"),
            Err(t!("vault.err_password_length"))
        );
        assert_eq!(
            check_new("abcdefg", "abcdefh"),
            Err(t!("vault.err_mismatch"))
        );
        assert!(check_new(&"x".repeat(21), &"x".repeat(21)).is_err());
    }

    /// Menu ☰ d'une boîte, sans autres boîtes ni catégories rangées.
    fn test_menu(b: &DeskBox, page: Page) -> Vec<Entry> {
        let targets = super::super::targets::Targets::default();
        super::super::menu::box_menu(
            b,
            page,
            &super::super::menu::MenuCx {
                others: &[],
                icon_size: 48.0,
                targets: &targets,
            },
        )
    }

    #[test]
    fn private_box_entry_follows_new_box() {
        let b = DeskBox::new("A");
        let mut state = DesktopState::default();
        state.boxes.push(b.clone());
        let mut entries = test_menu(&b, Page::Create);
        adjust_menu(&state, &b, Page::Create, &mut entries);
        let pos = |entries: &[Entry], f: &dyn Fn(&Action) -> bool| {
            entries
                .iter()
                .position(|e| matches!(e, Entry::Item { kind: EntryKind::Do(a), .. } if f(a)))
        };
        let new_box = pos(&entries, &|a| matches!(a, Action::NewBox { .. })).unwrap();
        let private = pos(&entries, &|a| {
            matches!(a, Action::Vault(VaultAction::NewPrivateBox { .. }))
        })
        .unwrap();
        assert_eq!(private, new_box + 1);

        // Une boîte privée existe déjà : plus d'entrée.
        let mut p = DeskBox::new("Privée");
        p.kind = BoxKind::Private;
        state.boxes.push(p.clone());
        let mut entries = test_menu(&b, Page::Create);
        adjust_menu(&state, &b, Page::Create, &mut entries);
        assert!(
            pos(&entries, &|a| matches!(
                a,
                Action::Vault(VaultAction::NewPrivateBox { .. })
            ))
            .is_none()
        );

        // Dans la boîte privée : changer le mot de passe (coffre existant).
        EXISTS.store(true, Ordering::Relaxed);
        let mut entries = test_menu(&p, Page::Main);
        adjust_menu(&state, &p, Page::Main, &mut entries);
        assert!(
            pos(&entries, &|a| matches!(
                a,
                Action::Vault(VaultAction::ChangePassword)
            ))
            .is_some()
        );
    }

    #[test]
    fn item_menu_offers_extract_and_trash() {
        let b = BoxId::new();
        let menu = item_menu(b, &["a.txt".to_owned()]);
        let kinds: Vec<_> = menu
            .iter()
            .filter_map(|e| match e {
                Entry::Item { kind, .. } => Some(kind.clone()),
                Entry::Separator => None,
            })
            .collect();
        assert!(
            kinds.contains(&EntryKind::Do(Action::Vault(VaultAction::Extract {
                names: vec!["a.txt".into()]
            })))
        );
        assert!(kinds.contains(&EntryKind::Do(Action::AskTrash {
            b,
            names: vec!["a.txt".into()]
        })));
        // Plusieurs éléments : pas de renommage.
        let menu = item_menu(b, &["a".to_owned(), "b".to_owned()]);
        assert!(!menu.iter().any(|e| matches!(
            e,
            Entry::Item {
                kind: EntryKind::Do(Action::StartRename { .. }),
                ..
            }
        )));
    }

    #[test]
    fn vault_items_become_box_items() {
        let info = item_info(&VaultItem {
            name: "photo.JPG".into(),
            is_dir: false,
            size: 12,
            modified: Some(1_700_000_000),
        });
        assert_eq!(info.category, tidy_core::category::Category::Image);
        assert_eq!(info.size, 12);
        assert!(info.modified.is_some());
        let dir = item_info(&VaultItem {
            name: "Dossier".into(),
            is_dir: true,
            size: 0,
            modified: None,
        });
        assert_eq!(dir.category, tidy_core::category::Category::Folder);
    }

    #[test]
    fn errors_never_show_secrets() {
        let text = error_text(&VaultError::WrongPassword);
        assert_eq!(text, t!("vault.err_wrong_password"));
        assert_eq!(
            error_kind(&VaultError::NotFound("secret.pdf".into())),
            "élément introuvable"
        );
    }
}
