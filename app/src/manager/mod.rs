//! Fenêtre principale (`tidy --manager`) : navigation latérale et pages de réglages,
//! dans la DA de Tappy (en-tête avec logo, cartes, choix lime).
//!
//! Pages : Accueil, Boîtes, Rangement, Actions rapides, Options et Diagnostic ; les
//! autres sont posées et annoncées. Au premier lancement (`onboarding_done` faux), la
//! fenêtre s'ouvre sur l'assistant ([`onboarding`]).
//!
//! La fenêtre **n'écrit jamais `desktop.json`** : elle le lit, et envoie des
//! [`DesktopCommand`] à l'agent ([`Manager::send`]). Les réglages (`config.json`)
//! s'écrivent avec [`Manager::save`], qui prévient l'agent.
//!
//! Processus à part : sa mémoire est rendue au système dès qu'on ferme la fenêtre.

mod appearance;
mod boxes;
mod layouts;
mod onboarding;
mod organization;
mod pages;
mod quick_actions;
mod report;
mod shortcuts;
/// Page Widgets (lot H). Nommée autrement que `tidy_ui::widgets`, importé ici.
#[path = "widgets.rs"]
mod widgets_page;

use std::collections::{BTreeSet, HashMap};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use tidy_core::category::Category;
use tidy_core::events::Event;
use tidy_core::{BoxId, DeskBox, DesktopCommand, DesktopState, Settings, Store};
use tidy_ui::{icons, t, theme, widgets};

use crate::ipc::{self, Request, Response, Status};

/// Pages de la fenêtre, dans l'ordre de la navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Boxes,
    Organization,
    QuickActions,
    Layouts,
    Wallpapers,
    Widgets,
    Search,
    Options,
    Diagnostic,
    /// Assistant de premier lancement (hors navigation ; `tidy --manager onboarding`).
    Onboarding,
}

impl Page {
    const ALL: [Page; 10] = [
        Page::Home,
        Page::Boxes,
        Page::Organization,
        Page::QuickActions,
        Page::Layouts,
        Page::Wallpapers,
        Page::Widgets,
        Page::Search,
        Page::Options,
        Page::Diagnostic,
    ];

    /// Page d'après son nom en argument (`tidy --manager options`).
    pub fn from_arg(arg: &str) -> Option<Page> {
        Some(match arg {
            "home" => Page::Home,
            "boxes" => Page::Boxes,
            // « boxes:<id> » : la page Boîtes, cette boîte dépliée (menu ☰ > Réglages…).
            a if boxes::box_from_arg(a).is_some() => Page::Boxes,
            "organization" => Page::Organization,
            "quick-actions" | "quick_actions" => Page::QuickActions,
            "layouts" => Page::Layouts,
            "wallpapers" => Page::Wallpapers,
            "widgets" => Page::Widgets,
            "search" => Page::Search,
            "options" => Page::Options,
            "diagnostic" => Page::Diagnostic,
            "onboarding" | "welcome" => Page::Onboarding,
            _ => return None,
        })
    }

    fn icon(self) -> &'static str {
        match self {
            Page::Home => icons::HOME,
            Page::Boxes => icons::BOX,
            Page::Organization => icons::AUTO_TIDY,
            Page::QuickActions => icons::QUICK_ACTIONS,
            Page::Layouts => icons::SNAPSHOT,
            Page::Wallpapers => icons::WALLPAPER,
            Page::Widgets => icons::WIDGETS,
            Page::Search => icons::SEARCH,
            Page::Options => icons::OPTIONS,
            Page::Diagnostic => icons::DIAGNOSTIC,
            Page::Onboarding => icons::ROCKET,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Page::Home => t!("manager.home"),
            Page::Boxes => t!("manager.boxes"),
            Page::Organization => t!("manager.organization"),
            Page::QuickActions => t!("manager.quick_actions"),
            Page::Layouts => t!("manager.layouts"),
            Page::Wallpapers => t!("manager.wallpapers"),
            Page::Widgets => t!("manager.widgets"),
            Page::Search => t!("manager.search"),
            Page::Options => t!("manager.options"),
            Page::Diagnostic => t!("manager.diagnostic"),
            Page::Onboarding => t!("onboarding.title"),
        }
    }
}

pub fn run(store: Store, page: Option<Page>) -> Result<()> {
    let settings: Settings = store.load();
    crate::lang::apply(settings.language);

    let (tx, events) = mpsc::channel();
    let ctx_cell: Arc<OnceLock<egui::Context>> = Arc::default();
    let wake = Arc::clone(&ctx_cell);
    if let Err(e) = ipc::subscribe(
        &store,
        move |event| {
            // Arrêt immédiat : une fenêtre cachée ne verrait pas l'événement avant
            // d'être redessinée. Tout est déjà enregistré.
            if event == Event::Shutdown {
                std::process::exit(0);
            }
            let _ = tx.send(event);
            if let Some(ctx) = wake.get() {
                ctx.request_repaint();
            }
        },
        || {},
    ) {
        log::warn!("pas d'agent ({e:#}) : fenêtre principale seule");
    }

    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Tidy")
            .with_inner_size([1000.0, 700.0])
            .with_min_inner_size([760.0, 520.0]),
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "Tidy",
        native,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            let _ = ctx_cell.set(cc.egui_ctx.clone());
            Ok(Box::new(Manager::new(store, settings, events, page)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("fenêtre principale : {e}"))
}

/// Action qui attend une confirmation (modale).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirm {
    /// Supprimer une boîte (aucun fichier supprimé).
    DeleteBox(BoxId),
    /// Ranger tous les éléments, même ceux déjà rangés à la main.
    ApplyToAll,
    /// Annuler le rangement automatique.
    UndoOrganize,
}

/// État de saisie des pages (textes en cours, sections ouvertes).
#[derive(Default)]
pub struct Drafts {
    /// Nom de boîte en cours de saisie.
    pub box_names: HashMap<BoxId, String>,
    /// Boîte dont le détail est ouvert (page Boîtes).
    pub open_box: Option<BoxId>,
    /// Catégories dont les extensions sont ouvertes (page Rangement).
    pub open_categories: BTreeSet<Category>,
    /// Extension en cours de saisie, par catégorie.
    pub new_extension: HashMap<Category, String>,
}

pub struct Manager {
    pub store: Store,
    pub settings: Settings,
    /// État du bureau, lu dans `desktop.json` (jamais écrit ici).
    pub desktop: DesktopState,
    pub page: Page,
    events: Receiver<Event>,
    pub status: Option<Status>,
    status_at: Option<Instant>,
    /// Dernière erreur à montrer.
    pub error: Option<String>,
    /// Message de réussite, effacé après quelques secondes.
    pub notice: Option<(String, Instant)>,
    pub confirm: Option<Confirm>,
    pub drafts: Drafts,
    pub onboarding: Option<onboarding::Wizard>,
    /// Boîtes modifiées ici, à envoyer quand la souris est relâchée (curseurs).
    dirty_boxes: BTreeSet<BoxId>,
    /// Réglages modifiés par un curseur, à enregistrer quand la souris est relâchée.
    settings_dirty: bool,
    /// Dernière commande envoyée au bureau : on attend un peu avant de relire le
    /// fichier, le temps que le bureau l'enregistre.
    sent_at: Option<Instant>,
    desktop_read_at: Instant,
}

/// Fréquence de mise à jour de l'état de l'agent tant que la fenêtre est ouverte.
const STATUS_EVERY: Duration = Duration::from_secs(2);
/// Relecture de `desktop.json` (modifié par le bureau : nouvelle boîte, renommage…).
const DESKTOP_EVERY: Duration = Duration::from_secs(3);
/// Durée d'affichage d'un message de réussite.
const NOTICE_FOR: Duration = Duration::from_secs(4);

impl Manager {
    fn new(store: Store, settings: Settings, events: Receiver<Event>, page: Option<Page>) -> Self {
        let desktop = store.load();
        // `--manager boxes:<id>` : boîte à déplier.
        let open_box = std::env::args()
            .nth(2)
            .as_deref()
            .and_then(boxes::box_from_arg);
        // Une page demandée explicitement (`--manager boxes`) passe avant l'assistant.
        let onboarding = match page {
            Some(Page::Onboarding) => true,
            Some(_) => false,
            None => !settings.onboarding_done,
        }
        .then(onboarding::Wizard::default);
        let page = match page {
            Some(Page::Onboarding) | None => Page::Home,
            Some(p) => p,
        };
        Self {
            store,
            settings,
            desktop,
            page,
            events,
            status: None,
            status_at: None,
            error: None,
            notice: None,
            confirm: None,
            drafts: Drafts {
                open_box,
                ..Drafts::default()
            },
            onboarding,
            dirty_boxes: BTreeSet::new(),
            settings_dirty: false,
            sent_at: None,
            desktop_read_at: Instant::now(),
        }
    }

    /// Enregistre les réglages et prévient l'agent.
    pub fn save(&mut self) {
        self.settings_dirty = false;
        match self.store.save(&self.settings) {
            Ok(()) => {
                crate::lang::apply(self.settings.language);
                if ipc::request(&self.store, &Request::ReloadSettings).is_err() {
                    self.error = Some(t!("error.agent_unreachable").into());
                }
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    /// Réglages modifiés par un curseur : enregistrés quand la souris est relâchée.
    pub fn save_later(&mut self) {
        self.settings_dirty = true;
    }

    /// Requête à l'agent. Rend `true` si elle a abouti ; sinon l'erreur est affichée.
    pub fn request(&mut self, req: Request) -> bool {
        match ipc::request(&self.store, &req) {
            Ok(Response::Error { message }) => {
                self.error = Some(message);
                false
            }
            Ok(_) => {
                self.status_at = None;
                true
            }
            Err(_) => {
                self.error = Some(t!("error.agent_unreachable").into());
                false
            }
        }
    }

    /// Modifie l'état du bureau : appliquée tout de suite ici (affichage), envoyée à
    /// l'agent qui la relaie au bureau. En cas d'échec, l'état est relu du fichier.
    pub fn send(&mut self, command: DesktopCommand) -> bool {
        self.desktop.apply(command.clone());
        self.sent_at = Some(Instant::now());
        let ok = self.request(Request::Desktop { command });
        if !ok {
            self.desktop = self.store.load();
        }
        ok
    }

    /// Modifie une boîte ici ; elle sera envoyée au bureau dès que la souris est
    /// relâchée (un curseur ne doit pas envoyer une commande par image).
    pub fn edit_box(&mut self, id: BoxId, edit: impl FnOnce(&mut DeskBox)) {
        if let Some(b) = self.desktop.find_box_mut(id) {
            edit(b);
            self.dirty_boxes.insert(id);
        }
    }

    pub fn show_notice(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), Instant::now()));
    }

    /// Envoie les modifications en attente quand plus rien n'est en cours de glisser.
    fn flush(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        for id in std::mem::take(&mut self.dirty_boxes) {
            if let Some(b) = self.desktop.find_box(id).cloned() {
                self.send(DesktopCommand::UpsertBox { b });
            }
        }
        if self.settings_dirty {
            self.save();
        }
    }

    /// Relit `desktop.json` de temps en temps (le bureau le modifie aussi), sauf
    /// pendant une saisie ou juste après un envoi.
    fn refresh_desktop(&mut self, ctx: &egui::Context, force: bool) {
        let busy = !self.dirty_boxes.is_empty()
            || ctx.memory(|m| m.focused().is_some())
            || ctx.input(|i| i.pointer.any_down())
            || self.sent_at.is_some_and(|t| t.elapsed() < DESKTOP_EVERY);
        if busy || (!force && self.desktop_read_at.elapsed() < DESKTOP_EVERY) {
            return;
        }
        self.desktop = self.store.load();
        self.desktop_read_at = Instant::now();
        if matches!(self.page, Page::Boxes | Page::Organization | Page::Widgets) {
            ctx.request_repaint_after(DESKTOP_EVERY);
        }
    }

    fn refresh_status(&mut self, ctx: &egui::Context) {
        if self.status_at.is_some_and(|t| t.elapsed() < STATUS_EVERY) {
            return;
        }
        self.status = match ipc::request(&self.store, &Request::Status) {
            Ok(Response::Status(s)) => Some(s),
            _ => None,
        };
        self.status_at = Some(Instant::now());
        ctx.request_repaint_after(STATUS_EVERY);
    }

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::FocusManager { page } => {
                    if let Some(page) = page.as_deref().and_then(Page::from_arg) {
                        self.page = page;
                    }
                    if let Some(id) = page.as_deref().and_then(boxes::box_from_arg) {
                        self.drafts.open_box = Some(id);
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                Event::SettingsChanged => {
                    if !self.settings_dirty {
                        self.settings = self.store.load();
                        crate::lang::apply(self.settings.language);
                    }
                }
                Event::DesktopChanged => self.refresh_desktop(ctx, true),
                Event::Shutdown => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                _ => {}
            }
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            widgets::app_header(ui, t!("manager.title"));
            ui.add_space(8.0);
            match &self.status {
                Some(_) => widgets::badge(ui, theme::ACCENT, t!("manager.agent_running")),
                None => widgets::badge(ui, theme::WARNING, t!("manager.agent_stopped")),
            };
            ui.label(widgets::dim(t!("manager.subtitle")));
        });
    }

    fn nav(&mut self, ui: &mut egui::Ui) {
        let selected = Page::ALL.iter().position(|p| *p == self.page).unwrap_or(0);
        let items = Page::ALL.map(|p| (p.icon(), p.label()));
        if let Some(i) = widgets::nav(ui, selected, items) {
            self.page = Page::ALL[i];
            self.refresh_desktop(ui.ctx(), true);
        }
    }
}

impl eframe::App for Manager {
    /// Tourne même quand la fenêtre est cachée derrière d'autres.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_events(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.refresh_status(&ctx);
        self.refresh_desktop(&ctx, false);
        if self
            .notice
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() > NOTICE_FOR)
        {
            self.notice = None;
        }
        if self.notice.is_some() {
            ctx.request_repaint_after(NOTICE_FOR);
        }
        egui::Panel::top("en-tête")
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(ui, |ui| self.header(ui));
        if self.onboarding.is_some() {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(theme::BG).inner_margin(32))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink(false)
                        .show(ui, |ui| onboarding::show(self, ui));
                });
            self.flush(&ctx);
            return;
        }
        egui::Panel::left("navigation")
            .resizable(false)
            .exact_size(210.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(12)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER)),
            )
            .show(ui, |ui| self.nav(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::symmetric(32, 24)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| pages::show(self, ui));
            });
        pages::confirmation(self, &ctx);
        self.flush(&ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager(dir: &std::path::Path) -> Manager {
        let store = Store::open(dir).unwrap();
        let mut desktop = DesktopState::default();
        let mut b = DeskBox::new("Documents");
        b.hotkey = Some("control+alt+KeyD".into());
        desktop.boxes.push(b);
        desktop.boxes.push(DeskBox::new(""));
        store.save(&desktop).unwrap();
        let settings = Settings {
            onboarding_done: true,
            ..Settings::default()
        };
        let (_tx, rx) = mpsc::channel();
        Manager::new(store, settings, rx, Some(Page::Home))
    }

    /// Dessine une image de la fenêtre, sans écran.
    fn frame(m: &mut Manager, ctx: &egui::Context) {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 700.0),
            )),
            ..Default::default()
        };
        let out = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                if m.onboarding.is_some() {
                    onboarding::show(m, ui);
                } else {
                    pages::show(m, ui);
                }
            });
            pages::confirmation(m, ui.ctx());
        });
        out.drop_without_applying_deltas();
    }

    /// Chaque page (détails ouverts) et chaque étape de l'assistant se dessinent sans
    /// erreur, sans agent.
    #[test]
    fn every_page_draws() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = manager(dir.path());
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        m.drafts.open_box = m.desktop.boxes.first().map(|b| b.id);
        m.drafts
            .open_categories
            .extend([Category::Image, Category::Folder]);
        for page in Page::ALL {
            m.page = page;
            frame(&mut m, &ctx);
        }
        for step in 0..3 {
            m.onboarding = Some(onboarding::Wizard {
                step,
                ..Default::default()
            });
            frame(&mut m, &ctx);
        }
        m.onboarding = None;
        m.confirm = Some(Confirm::DeleteBox(m.desktop.boxes[0].id));
        frame(&mut m, &ctx);
        assert!(m.confirm.is_some(), "la confirmation attend une réponse");
    }

    #[test]
    fn explicit_page_skips_the_onboarding() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let (_tx, rx) = mpsc::channel();
        let m = Manager::new(store.clone(), Settings::default(), rx, Some(Page::Boxes));
        assert!(m.onboarding.is_none());
        assert_eq!(m.page, Page::Boxes);
        let (_tx, rx) = mpsc::channel();
        let m = Manager::new(store.clone(), Settings::default(), rx, None);
        assert!(m.onboarding.is_some(), "premier lancement : l'assistant");
        let (_tx, rx) = mpsc::channel();
        let done = Settings {
            onboarding_done: true,
            ..Settings::default()
        };
        let m = Manager::new(store, done, rx, Some(Page::Onboarding));
        assert!(m.onboarding.is_some());
        assert_eq!(Page::from_arg("quick-actions"), Some(Page::QuickActions));
    }

    /// Sans agent, une commande échoue proprement et l'état est relu du fichier :
    /// la fenêtre n'écrit jamais `desktop.json`.
    #[test]
    fn commands_without_agent_show_an_error_and_keep_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = manager(dir.path());
        let before = m.desktop.clone();
        assert!(!m.send(DesktopCommand::UpsertBox {
            b: DeskBox::new("Nouvelle")
        }));
        assert!(m.error.is_some());
        assert_eq!(m.desktop, before);
        let on_disk: DesktopState = m.store.load();
        assert_eq!(on_disk, before);
    }
}
