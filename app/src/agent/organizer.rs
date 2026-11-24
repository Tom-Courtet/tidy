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

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use jiff::Timestamp;
use tidy_core::events::{Event, EventBus};
use tidy_core::organize::{self, DefaultBoxNames, Scope};
use tidy_core::rules::MatcherRegistry;
use tidy_core::scan::scan_entries;
use tidy_core::store::{desktop_dir, write_atomic};
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

/// Relaie une commande au processus du bureau, ou l'applique au fichier s'il ne tourne
/// pas.
pub fn route(
    store: &Store,
    bus: &EventBus,
    desktop_running: bool,
    command: DesktopCommand,
) -> Response {
    if desktop_running {
        bus.publish(Event::Desktop { command });
        return Response::Ok;
    }
    let _writing = WRITING.lock().unwrap_or_else(|p| p.into_inner());
    let mut state: DesktopState = store.load();
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
    match try_apply_rules(store, bus, all, desktop_dir().as_deref()) {
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
    desktop: Option<&Path>,
) -> Result<(), String> {
    let dir = desktop.ok_or_else(|| t!("organize.no_desktop").to_owned())?;
    let entries = scan_entries(dir).map_err(|e| t!("organize.scan_failed", error = e))?;
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
        if let Response::Error { message } = route(store, bus, running, command) {
            return Err(message);
        }
    }
    sync_icons(store, running);
    Ok(())
}

/// Icônes rangées masquées, les autres rendues (Windows). À part : l'Explorateur peut
/// répondre lentement, et le bureau doit d'abord avoir enregistré l'état.
fn sync_icons(store: &Store, desktop_running: bool) {
    // Jamais pendant les tests : ce serait le vrai bureau de la machine.
    if cfg!(test) {
        return;
    }
    let store = store.clone();
    std::thread::spawn(move || {
        if desktop_running {
            std::thread::sleep(DESKTOP_SAVE_DELAY);
        }
        match crate::icons::sync(&store, &[]) {
            Some(Ok(r)) if r.hidden + r.restored > 0 => log::info!(
                "icônes rangées : {} masquées, {} rendues",
                r.hidden,
                r.restored
            ),
            Some(Err(e)) => log::warn!("icônes rangées : {e:#}"),
            _ => {}
        }
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
        try_apply_rules(&store, &bus, true, Some(desktop.path())).unwrap();
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
        try_apply_rules(&store, &bus, false, Some(desktop.path())).unwrap();
        let state: DesktopState = store.load();
        assert!(state.box_of(&ItemRef("musique.mp3".into())).is_some());

        assert_eq!(undo(&store, &bus), Response::Ok);
        assert_eq!(store.load::<DesktopState>(), before);
        assert!(!data.path().join(BEFORE_ORGANIZE_FILE).exists());
        assert!(try_apply_rules(&store, &bus, true, None).is_err());
    }
}
