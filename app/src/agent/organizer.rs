//! Rangement : règles, nouveaux fichiers du bureau, « Tout annuler ».
//!
//! - [`apply_rules`] : « Appliquer aux non affectés » ou « à tous » (démarrage rapide
//!   compris), avec une copie de l'état dans `desktop.before-organize.json` avant le
//!   tout premier rangement ;
//! - [`undo`] : rend l'état de cette copie ;
//! - surveillance du dossier du bureau : [`super::watcher`], démarrée par [`start`].
//!
//! Le calcul est dans `tidy_core::organize` (pur, testé). Toute modification de l'état
//! passe par [`route`] (un seul écrivain : le bureau), puis les icônes rangées sont
//! resynchronisées (`crate::icons::sync`, Windows).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use jiff::Timestamp;
use tidy_core::events::{Event, EventBus};
use tidy_core::organize::{self, DefaultBoxNames, Scope};
use tidy_core::rules::MatcherRegistry;
use tidy_core::scan::scan_desktop;
use tidy_core::store::{desktop_dirs, write_atomic};
use tidy_core::{DesktopCommand, DesktopState, Settings, Store};
use tidy_ui::t;

use super::supervisor::Shared;
use crate::ipc::Response;

/// Copie de l'état prise avant le tout premier rangement (dans le dossier de données).
pub const BEFORE_ORGANIZE_FILE: &str = "desktop.before-organize.json";

/// Délai laissé au processus du bureau pour appliquer et enregistrer les commandes
/// avant de resynchroniser les icônes rangées.
const DESKTOP_SAVE_DELAY: Duration = Duration::from_millis(1500);

/// État partagé de l'agent (le bureau tourne-t-il ?), connu après [`start`].
static SHARED: OnceLock<Arc<Mutex<Shared>>> = OnceLock::new();

/// Un seul rangement à la fois (requêtes et surveillance du bureau).
static ORGANIZING: Mutex<()> = Mutex::new(());

/// Une seule écriture de `desktop.json` à la fois quand le bureau ne tourne pas.
static WRITING: Mutex<()> = Mutex::new(());

/// Démarre la surveillance du dossier du bureau.
pub fn start(store: Store, bus: EventBus, shared: Arc<Mutex<Shared>>) {
    let _ = SHARED.set(shared);
    super::watcher::spawn(store, bus);
}

/// Le processus du bureau tourne-t-il ?
fn desktop_running() -> bool {
    SHARED
        .get()
        .is_some_and(|s| s.lock().is_ok_and(|s| s.desktop_running))
}

/// Délai de secours : un bureau lancé depuis plus longtemps sans avoir signalé
/// `DesktopReady` (bureau masqué au démarrage, fenêtre jamais posée) est abonné depuis
/// longtemps ; les commandes en attente lui sont alors relayées.
pub const READY_FALLBACK: Duration = Duration::from_secs(20);

/// Relaie une commande au processus du bureau, ou l'applique au fichier s'il ne tourne
/// pas. Pendant le démarrage du bureau (lancé, pas encore abonné), la commande attend
/// dans l'état partagé : elle sera relayée à `DesktopReady` ([`desktop_ready`]), ou
/// appliquée au fichier si le bureau meurt avant ([`desktop_stopped`]).
pub fn route(store: &Store, bus: &EventBus, command: DesktopCommand) -> Response {
    route_with(SHARED.get().map(|s| &**s), store, bus, command)
}

fn route_with(
    shared: Option<&Mutex<Shared>>,
    store: &Store,
    bus: &EventBus,
    command: DesktopCommand,
) -> Response {
    if let Some(shared) = shared {
        let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
        if s.desktop_running {
            // Sous le verrou : l'ordre est gardé avec le vidage de la file.
            if s.desktop_ready {
                bus.publish(Event::Desktop { command });
            } else {
                log::debug!("commande du bureau en attente (bureau en démarrage)");
                s.pending.push(command);
            }
            return Response::Ok;
        }
    }
    let _writing = WRITING.lock().unwrap_or_else(|p| p.into_inner());
    apply_to_file(store, bus, command)
}

/// Le bureau est abonné (`Request::DesktopReady`) : les commandes en attente lui sont
/// relayées, dans l'ordre.
pub fn desktop_ready(bus: &EventBus, shared: &Mutex<Shared>) {
    let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
    if !s.desktop_running {
        return;
    }
    s.desktop_ready = true;
    flush(bus, &mut s);
}

/// Secours : bureau lancé depuis plus de [`READY_FALLBACK`] sans `DesktopReady`.
pub fn flush_late(bus: &EventBus, shared: &Mutex<Shared>) {
    let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
    let late = s.started_at.is_some_and(|t| t.elapsed() >= READY_FALLBACK);
    if s.desktop_running && !s.desktop_ready && late && !s.pending.is_empty() {
        log::warn!("bureau sans « prêt » depuis {READY_FALLBACK:?} : commandes relayées");
        flush(bus, &mut s);
    }
}

fn flush(bus: &EventBus, s: &mut Shared) {
    for command in s.pending.drain(..) {
        bus.publish(Event::Desktop { command });
    }
}

/// Le bureau vient d'être lancé : les commandes attendront son `DesktopReady`.
pub fn desktop_started(shared: &Mutex<Shared>) {
    let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
    s.desktop_running = true;
    s.desktop_ready = false;
    s.started_at = Some(std::time::Instant::now());
}

/// Le bureau s'est arrêté (ou a été arrêté) : les commandes qu'il n'a pas reçues sont
/// appliquées au fichier, dans l'ordre, avant toute nouvelle commande.
pub fn desktop_stopped(store: &Store, bus: &EventBus, shared: &Mutex<Shared>) {
    // [`WRITING`] d'abord : une commande arrivée entre-temps (bureau désormais arrêté)
    // attend que la file soit appliquée.
    let _writing = WRITING.lock().unwrap_or_else(|p| p.into_inner());
    let pending = {
        let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
        s.desktop_running = false;
        s.desktop_ready = false;
        s.started_at = None;
        std::mem::take(&mut s.pending)
    };
    if !pending.is_empty() {
        log::info!(
            "bureau arrêté avant d'être prêt : {} commande(s) appliquée(s) au fichier",
            pending.len()
        );
    }
    for command in pending {
        if let Response::Error { message } = apply_to_file(store, bus, command) {
            log::error!("commande du bureau perdue : {message}");
        }
    }
}

/// Applique une commande à `desktop.json` (bureau arrêté). L'appelant tient [`WRITING`].
fn apply_to_file(store: &Store, bus: &EventBus, command: DesktopCommand) -> Response {
    // Un fichier qu'on n'a pas pu lire n'est jamais réécrit (il serait remplacé par un
    // bureau vide).
    let mut state: DesktopState = match store.load_checked() {
        Ok(state) => state,
        Err(e) => {
            return Response::Error {
                message: format!("{e:#}"),
            };
        }
    };
    state.apply(command);
    match store.save(&state) {
        Ok(()) => {
            bus.publish(Event::DesktopChanged);
            Response::Ok
        }
        Err(e) => Response::Error {
            message: format!("{e:#}"),
        },
    }
}

/// Applique les règles : à tous les éléments (`all`, avec le démarrage rapide s'il n'y
/// a encore ni boîte ni règle) ou seulement aux non affectés.
pub fn apply_rules(store: &Store, bus: &EventBus, all: bool) -> Response {
    let _organizing = ORGANIZING.lock().unwrap_or_else(|p| p.into_inner());
    match try_apply_rules(store, bus, all, &desktop_dirs()) {
        Ok(()) => Response::Ok,
        Err(message) => {
            log::warn!("rangement : {message}");
            Response::Error { message }
        }
    }
}

fn try_apply_rules(
    store: &Store,
    bus: &EventBus,
    all: bool,
    desktop: &[PathBuf],
) -> Result<(), String> {
    if desktop.is_empty() {
        return Err(t!("organize.no_desktop").to_owned());
    }
    let entries = scan_desktop(desktop).map_err(|e| t!("organize.scan_failed", error = e))?;
    let state: DesktopState = store.load();
    let mut settings: Settings = store.load();
    let scope = if all { Scope::All } else { Scope::Unassigned };
    let plan = organize::plan(
        &state,
        &settings.organization,
        &entries,
        scope,
        Some(&default_box_names()),
        &MatcherRegistry::default(),
        Timestamp::now(),
    );
    if plan.is_empty() {
        log::info!("rangement : rien à faire");
        return Ok(());
    }
    save_before_organize(store, &state)?;
    if let Some(organization) = plan.organization {
        settings.organization = organization;
        store
            .save(&settings)
            .map_err(|e| t!("organize.settings_failed", error = format!("{e:#}")))?;
        bus.publish(Event::SettingsChanged);
        log::info!("rangement : boîtes par défaut créées (démarrage rapide)");
    }
    log::info!(
        "rangement ({}) : {} élément(s) rangé(s), {} non affecté(s)",
        if all { "tous" } else { "non affectés" },
        plan.moved,
        plan.unassigned
    );
    send(store, bus, plan.commands)
}

/// Noms des boîtes du démarrage rapide, dans la langue de l'interface.
fn default_box_names() -> DefaultBoxNames {
    DefaultBoxNames {
        programs: t!("organize.box_programs").to_owned(),
        folders: t!("organize.box_folders").to_owned(),
        files: t!("organize.box_files").to_owned(),
    }
}

/// Copie de l'état avant le tout premier rangement (gardée jusqu'à « Tout annuler »).
fn save_before_organize(store: &Store, state: &DesktopState) -> Result<(), String> {
    let path = store.root().join(BEFORE_ORGANIZE_FILE);
    if path.exists() {
        return Ok(());
    }
    let error = |e: String| t!("organize.snapshot_failed", error = e);
    let text = serde_json::to_string_pretty(state).map_err(|e| error(e.to_string()))?;
    write_atomic(&path, text.as_bytes()).map_err(|e| error(format!("{e:#}")))
}

/// Annule le rangement : rend l'état d'avant le premier rangement. La copie est ensuite
/// retirée ; le prochain rangement en prendra une nouvelle.
pub fn undo(store: &Store, bus: &EventBus) -> Response {
    let _organizing = ORGANIZING.lock().unwrap_or_else(|p| p.into_inner());
    match try_undo(store, bus) {
        Ok(()) => Response::Ok,
        Err(message) => {
            log::warn!("annulation du rangement : {message}");
            Response::Error { message }
        }
    }
}

fn try_undo(store: &Store, bus: &EventBus) -> Result<(), String> {
    let path = store.root().join(BEFORE_ORGANIZE_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(t!("organize.nothing_to_undo").to_owned());
        }
        Err(e) => return Err(t!("organize.undo_unreadable", error = e)),
    };
    let state: DesktopState = serde_json::from_str::<serde_json::Value>(&text)
        .map_err(anyhow::Error::from)
        .and_then(tidy_core::store::migrate::<DesktopState>)
        .map_err(|e| t!("organize.undo_unreadable", error = format!("{e:#}")))?;
    send(
        store,
        bus,
        vec![DesktopCommand::Replace {
            state: Box::new(state),
        }],
    )?;
    if let Err(e) = std::fs::remove_file(&path) {
        log::warn!("{} non retiré : {e}", path.display());
    }
    log::info!("rangement annulé");
    Ok(())
}

/// Fait appliquer `commands` dans l'ordre, puis resynchronise les icônes rangées.
pub(super) fn send(
    store: &Store,
    bus: &EventBus,
    commands: Vec<DesktopCommand>,
) -> Result<(), String> {
    if commands.is_empty() {
        return Ok(());
    }
    let running = desktop_running();
    for command in commands {
        if let Response::Error { message } = route(store, bus, command) {
            return Err(message);
        }
    }
    sync_icons(store, running);
    Ok(())
}

/// Le superviseur a-t-il renoncé au bureau (trop d'arrêts) ?
fn desktop_gave_up() -> bool {
    SHARED
        .get()
        .is_some_and(|s| s.lock().is_ok_and(|s| s.gave_up))
}

/// Icônes rangées masquées, les autres rendues (Windows). À part : l'Explorateur peut
/// répondre lentement, et le bureau doit d'abord avoir enregistré l'état. Rien après
/// l'abandon du bureau par le superviseur : les icônes rendues resteraient sinon
/// masquées sans boîte pour les montrer.
fn sync_icons(store: &Store, desktop_running: bool) {
    // Jamais pendant les tests : ce serait le vrai bureau de la machine.
    if cfg!(test) {
        return;
    }
    let store = store.clone();
    std::thread::spawn(move || {
        if desktop_running {
            // Commandes en attente du démarrage du bureau : attendre qu'il les reçoive.
            let deadline = std::time::Instant::now() + READY_FALLBACK;
            while std::time::Instant::now() < deadline
                && SHARED.get().is_some_and(|s| {
                    s.lock()
                        .is_ok_and(|s| s.desktop_running && !s.pending.is_empty())
                })
            {
                std::thread::sleep(Duration::from_millis(200));
            }
            std::thread::sleep(DESKTOP_SAVE_DELAY);
        }
        if desktop_gave_up() {
            log::debug!("icônes rangées : bureau abandonné, rien n'est masqué");
            return;
        }
        crate::icons::log_report(crate::icons::sync(&store, &[]), true);
    });
}

/// Garde-fou de la surveillance : un seul rangement à la fois.
pub(super) fn organizing() -> std::sync::MutexGuard<'static, ()> {
    ORGANIZING.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidy_core::DeskBox;
    use tidy_core::model::ItemRef;

    fn unassigned(names: &[&str]) -> DesktopCommand {
        DesktopCommand::SetUnassigned {
            items: names.iter().map(|n| ItemRef((*n).into())).collect(),
        }
    }

    fn published(events: &std::sync::mpsc::Receiver<Event>) -> Vec<DesktopCommand> {
        events
            .try_iter()
            .filter_map(|e| match e {
                Event::Desktop { command } => Some(command),
                _ => None,
            })
            .collect()
    }

    /// Bureau lancé mais pas encore abonné : les commandes attendent, puis partent dans
    /// l'ordre à `DesktopReady`.
    #[test]
    fn commands_wait_for_desktop_ready() {
        let data = tempfile::tempdir().unwrap();
        let store = Store::open(data.path()).unwrap();
        let bus = EventBus::default();
        let events = bus.subscribe();
        let shared = Mutex::new(Shared::default());
        desktop_started(&shared);

        for name in ["a", "b"] {
            let r = route_with(Some(&shared), &store, &bus, unassigned(&[name]));
            assert_eq!(r, Response::Ok);
        }
        assert!(
            published(&events).is_empty(),
            "rien ne part avant le « prêt »"
        );
        assert_eq!(shared.lock().unwrap().pending.len(), 2);
        flush_late(&bus, &shared);
        assert!(published(&events).is_empty(), "pas encore en retard");

        desktop_ready(&bus, &shared);
        assert_eq!(published(&events), [unassigned(&["a"]), unassigned(&["b"])]);
        route_with(Some(&shared), &store, &bus, unassigned(&["c"]));
        assert_eq!(published(&events), [unassigned(&["c"])]);
        assert!(!data.path().join("desktop.json").exists());
    }

    /// Bureau mort avant d'être prêt : les commandes en attente vont au fichier.
    #[test]
    fn pending_commands_go_to_the_file_if_the_desktop_dies() {
        let data = tempfile::tempdir().unwrap();
        let store = Store::open(data.path()).unwrap();
        store.save(&DesktopState::default()).unwrap();
        let bus = EventBus::default();
        let events = bus.subscribe();
        let shared = Mutex::new(Shared::default());
        desktop_started(&shared);
        route_with(Some(&shared), &store, &bus, unassigned(&["a"]));
        route_with(Some(&shared), &store, &bus, unassigned(&["a", "b"]));

        desktop_stopped(&store, &bus, &shared);
        assert!(published(&events).is_empty());
        let state: DesktopState = store.load();
        assert_eq!(state.unassigned, [ItemRef("a".into()), ItemRef("b".into())]);
        assert!(shared.lock().unwrap().pending.is_empty());
        // Bureau arrêté : directement au fichier.
        route_with(Some(&shared), &store, &bus, unassigned(&[]));
        assert!(store.load::<DesktopState>().unassigned.is_empty());
    }

    /// Essai complet sur un faux bureau et des données jetables (bureau arrêté :
    /// l'agent écrit lui-même `desktop.json`).
    #[test]
    fn organize_then_undo_on_a_fake_desktop() {
        let data = tempfile::tempdir().unwrap();
        let desktop = tempfile::tempdir().unwrap();
        for name in ["Rapport.pdf", "photo.jpg", "Setup.exe"] {
            std::fs::write(desktop.path().join(name), b"x").unwrap();
        }
        std::fs::create_dir(desktop.path().join("Projets")).unwrap();
        let store = Store::open(data.path()).unwrap();
        let bus = EventBus::default();
        let events = bus.subscribe();
        let before = DesktopState::default();
        store.save(&before).unwrap();

        assert_eq!(
            undo(&store, &bus),
            Response::Error {
                message: t!("organize.nothing_to_undo").to_owned()
            }
        );
        try_apply_rules(&store, &bus, true, &[desktop.path().to_path_buf()]).unwrap();
        let state: DesktopState = store.load();
        assert_eq!(state.boxes.len(), 3, "démarrage rapide");
        let boxed: Vec<&ItemRef> = state.boxes.iter().flat_map(DeskBox::items).collect();
        assert_eq!(boxed.len(), 4);
        assert!(data.path().join(BEFORE_ORGANIZE_FILE).exists());
        let settings: Settings = store.load();
        assert_eq!(settings.organization.categories.len(), 11);
        assert!(events.try_iter().any(|e| e == Event::SettingsChanged));
        // Les fichiers n'ont pas bougé.
        assert!(desktop.path().join("Rapport.pdf").exists());

        // Un nouveau fichier, puis « Appliquer aux non affectés » : la copie d'avant le
        // premier rangement n'est pas remplacée.
        std::fs::write(desktop.path().join("musique.mp3"), b"x").unwrap();
        try_apply_rules(&store, &bus, false, &[desktop.path().to_path_buf()]).unwrap();
        let state: DesktopState = store.load();
        assert!(state.box_of(&ItemRef("musique.mp3".into())).is_some());

        assert_eq!(undo(&store, &bus), Response::Ok);
        assert_eq!(store.load::<DesktopState>(), before);
        assert!(!data.path().join(BEFORE_ORGANIZE_FILE).exists());
        assert!(try_apply_rules(&store, &bus, true, &[]).is_err());
    }
}
