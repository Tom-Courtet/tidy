//! Dispositions côté agent : instantanés automatiques, demandes du module `layouts`
//! (fenêtre principale), raccourcis des dispositions nommées et menu de la zone de
//! notification.
//!
//! - **Instantanés automatiques** : un fil relève la date de `desktop.json` (écrit par le
//!   seul processus du bureau, ou par l'agent quand le bureau ne tourne pas). Après un
//!   changement, il attend [`QUIET`] de calme (au plus [`MAX_WAIT`]) et laisse au moins
//!   [`MIN_INTERVAL`] entre deux instantanés ; un état identique au dernier instantané
//!   n'est pas repris (`tidy_core::layouts::Layouts::take`). Relever une date est sûr
//!   et ne coûte presque rien ; les événements du bus ne suffiraient pas (le bureau
//!   enregistre ses déplacements sans prévenir l'agent).
//! - **Restaurer** (instantané ou disposition nommée) : un instantané de sécurité de
//!   l'état actuel, puis une `DesktopCommand::Replace` routée comme les autres
//!   ([`super::organizer::send`]) : le processus du bureau reste le seul écrivain.
//! - L'agent est le **seul écrivain** du dossier `snapshots/` ; un verrou fait passer
//!   les demandes une à une.
//!
//! Actions du module (`Request::Module { module: "layouts", action, data }`) :
//! `list`, `take {name}`, `restore {id}`, `restore_previous`, `rename {id, name}`,
//! `delete {id}`, `prune`, `save_layout {name}`, `update_layout {id}`,
//! `apply_layout {id}`, `rename_layout {id, name}`, `delete_layout {id}`,
//! `set_layout_hotkey {id, hotkey}`. Après un changement, l'agent publie
//! `Event::Module { module: "layouts", action: "changed" }`.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use jiff::Timestamp;
use serde_json::{Value, json};
use tidy_core::events::{Event, EventBus};
use tidy_core::layouts::{
    Layouts, SnapshotKind, SnapshotReason, Taken, Uuid, fingerprint, load_state, peek_index,
    restored_state,
};
use tidy_core::store::{Versioned, migrate};
use tidy_core::{DesktopCommand, DesktopState, Settings, Store};
use tidy_ui::t;

use super::organizer;
use crate::ipc::Response;

/// Nom du module dans les messages.
pub const MODULE: &str = "layouts";

/// Fréquence de relevé de `desktop.json`.
const POLL: Duration = Duration::from_secs(1);
/// Calme attendu après un changement avant de prendre l'instantané.
const QUIET: Duration = Duration::from_secs(3);
/// Attente maximale (changements continus) avant de prendre l'instantané.
const MAX_WAIT: Duration = Duration::from_secs(30);
/// Écart minimal entre deux instantanés automatiques.
const MIN_INTERVAL: Duration = Duration::from_secs(5);

/// Une demande à la fois (fil des instantanés, socket, raccourcis, menu).
static BUSY: Mutex<()> = Mutex::new(());

/// Prévient le thread principal que les dispositions nommées ont changé (raccourcis et
/// menu à refaire).
static ON_NAMED_CHANGED: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Empreinte de l'état qui vient d'être restauré : le bureau va l'enregistrer, et cet
/// enregistrement n'est pas un nouveau changement à garder.
static RESTORED: Mutex<Option<u64>> = Mutex::new(None);

/// Lance la prise des instantanés automatiques. `on_named_changed` est appelé (depuis
/// n'importe quel fil) quand les dispositions nommées changent.
pub fn start(store: Store, bus: EventBus, on_named_changed: impl Fn() + Send + Sync + 'static) {
    let _ = ON_NAMED_CHANGED.set(Box::new(on_named_changed));
    let spawned = std::thread::Builder::new()
        .name("instantanés".into())
        .spawn(move || watch(&store, &bus));
    if let Err(e) = spawned {
        log::error!("instantanés automatiques impossibles : {e}");
    }
}

/// Date de modification de `desktop.json`.
fn modified(store: &Store) -> Option<SystemTime> {
    std::fs::metadata(store.path_of::<DesktopState>())
        .and_then(|m| m.modified())
        .ok()
}

/// Boucle du fil des instantanés.
fn watch(store: &Store, bus: &EventBus) {
    auto_snapshot(store, bus);
    let mut last = modified(store);
    let mut dirty_since: Option<Instant> = None;
    let mut changed_at = Instant::now();
    let mut taken_at = Instant::now();
    loop {
        std::thread::sleep(POLL);
        let now = modified(store);
        if now != last {
            last = now;
            changed_at = Instant::now();
            dirty_since.get_or_insert(changed_at);
        }
        let Some(since) = dirty_since else {
            continue;
        };
        let settled = changed_at.elapsed() >= QUIET || since.elapsed() >= MAX_WAIT;
        if settled && taken_at.elapsed() >= MIN_INTERVAL {
            auto_snapshot(store, bus);
            dirty_since = None;
            taken_at = Instant::now();
        }
    }
}

/// Lit `desktop.json` sans jamais le toucher (c'est le fichier du bureau).
fn read_desktop(store: &Store) -> Option<DesktopState> {
    let path = store.path_of::<DesktopState>();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(DesktopState::default()),
        Err(e) => {
            log::warn!("instantanés : lecture de {} : {e}", path.display());
            return None;
        }
    };
    serde_json::from_str::<Value>(&text)
        .map_err(anyhow::Error::from)
        .and_then(migrate::<DesktopState>)
        .inspect_err(|e| {
            log::warn!(
                "instantanés : {} illisible : {e:#}",
                DesktopState::FILE_NAME
            )
        })
        .ok()
}

/// Prend un instantané automatique si le réglage le demande et si la disposition a
/// changé depuis le dernier.
fn auto_snapshot(store: &Store, bus: &EventBus) {
    let _busy = BUSY.lock().unwrap_or_else(|p| p.into_inner());
    let settings: Settings = store.load();
    if !settings.snapshots.auto {
        return;
    }
    let Some(state) = read_desktop(store) else {
        return;
    };
    let layouts = Layouts::new(store.clone());
    // Un bureau vide n'a rien à garder tant qu'aucun instantané n'existe.
    if state.boxes.is_empty() && state.widgets.is_empty() && layouts.index().snapshots.is_empty() {
        return;
    }
    {
        let mut restored = RESTORED.lock().unwrap_or_else(|p| p.into_inner());
        if restored.take() == Some(fingerprint(&state)) {
            return;
        }
    }
    match layouts.take(
        &state,
        SnapshotKind::Auto,
        SnapshotReason::Changed,
        "",
        settings.snapshots.keep,
        Timestamp::now(),
    ) {
        Ok(Taken::New(meta)) => {
            log::info!("instantané automatique pris ({:?})", meta.kind);
            publish_changed(bus, false);
        }
        Ok(Taken::Duplicate) => {}
        Err(e) => log::warn!("instantané automatique impossible : {e:#}"),
    }
}

/// Prévient les abonnés (fenêtre principale) et, si les dispositions nommées ont
/// changé, le thread principal.
fn publish_changed(bus: &EventBus, named: bool) {
    bus.publish(Event::Module {
        module: MODULE.into(),
        action: "changed".into(),
        data: Value::Null,
    });
    if named && let Some(notify) = ON_NAMED_CHANGED.get() {
        notify();
    }
}

/// Traite une demande du module `layouts`.
pub fn handle(store: &Store, bus: &EventBus, action: &str, data: Value) -> Response {
    let _busy = BUSY.lock().unwrap_or_else(|p| p.into_inner());
    match run(store, bus, action, &data) {
        Ok(response) => response,
        Err(message) => {
            log::warn!("dispositions ({action}) : {message}");
            Response::Error { message }
        }
    }
}

fn id_of(data: &Value) -> Result<Uuid, String> {
    data.get("id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| t!("layouts.error_missing").to_owned())
}

fn text_of<'a>(data: &'a Value, key: &str) -> &'a str {
    data.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn failed(e: anyhow::Error) -> String {
    t!("layouts.error", error = format!("{e:#}"))
}

fn current_state(store: &Store) -> Result<DesktopState, String> {
    read_desktop(store).ok_or_else(|| t!("layouts.error_desktop_unreadable").to_owned())
}

fn run(store: &Store, bus: &EventBus, action: &str, data: &Value) -> Result<Response, String> {
    let layouts = Layouts::new(store.clone());
    let keep = store.load::<Settings>().snapshots.keep;
    let now = Timestamp::now();
    let named = match action {
        "list" => {
            let index = serde_json::to_value(layouts.index()).map_err(|e| e.to_string())?;
            return Ok(Response::Data { data: index });
        }
        "take" => {
            let state = current_state(store)?;
            layouts
                .take(
                    &state,
                    SnapshotKind::Manual,
                    SnapshotReason::User,
                    text_of(data, "name"),
                    keep,
                    now,
                )
                .map_err(failed)?;
            false
        }
        "restore" => {
            let id = id_of(data)?;
            let state = load_state(store, id).map_err(failed)?;
            restore(store, bus, &layouts, &state, keep)?;
            false
        }
        "restore_previous" => {
            let current = current_state(store)?;
            let index = layouts.index();
            let Some(previous) = index.previous(fingerprint(&current)) else {
                return Err(t!("layouts.error_no_previous").to_owned());
            };
            let state = load_state(store, previous.id).map_err(failed)?;
            restore(store, bus, &layouts, &state, keep)?;
            false
        }
        "rename" => {
            layouts
                .rename_snapshot(id_of(data)?, text_of(data, "name"))
                .map_err(failed)?;
            false
        }
        "delete" => {
            layouts.delete_snapshot(id_of(data)?).map_err(failed)?;
            false
        }
        "prune" => {
            layouts.prune_to(keep).map_err(failed)?;
            false
        }
        "save_layout" => {
            let state = current_state(store)?;
            let id = layouts
                .save_layout(text_of(data, "name"), &state, now)
                .map_err(failed)?;
            publish_changed(bus, true);
            return Ok(Response::Data {
                data: json!({ "id": id }),
            });
        }
        "update_layout" => {
            let state = current_state(store)?;
            layouts
                .update_layout(id_of(data)?, &state, now)
                .map_err(failed)?;
            false
        }
        "apply_layout" => {
            let id = id_of(data)?;
            let state = load_state(store, id).map_err(failed)?;
            restore(store, bus, &layouts, &state, keep)?;
            false
        }
        "rename_layout" => {
            layouts
                .rename_layout(id_of(data)?, text_of(data, "name"))
                .map_err(failed)?;
            true
        }
        "delete_layout" => {
            layouts.delete_layout(id_of(data)?).map_err(failed)?;
            true
        }
        "set_layout_hotkey" => {
            let hotkey = data
                .get("hotkey")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            layouts
                .set_layout_hotkey(id_of(data)?, hotkey)
                .map_err(failed)?;
            true
        }
        other => return Err(t!("layouts.error_unknown", action = other)),
    };
    publish_changed(bus, named);
    Ok(Response::Ok)
}

/// Restaure `snapshot` : instantané de sécurité de l'état actuel, puis remplacement de
/// l'état par le processus du bureau. Les éléments qui ne sont plus rangés reviennent
/// au bureau ; aucun fichier n'est touché.
fn restore(
    store: &Store,
    bus: &EventBus,
    layouts: &Layouts,
    snapshot: &DesktopState,
    keep: usize,
) -> Result<(), String> {
    let current = current_state(store)?;
    layouts
        .take(
            &current,
            SnapshotKind::Auto,
            SnapshotReason::BeforeRestore,
            "",
            keep,
            Timestamp::now(),
        )
        .map_err(|e| t!("layouts.error_safety", error = format!("{e:#}")))?;
    let state = restored_state(&current, snapshot);
    *RESTORED.lock().unwrap_or_else(|p| p.into_inner()) = Some(fingerprint(&state));
    organizer::send(
        store,
        bus,
        vec![DesktopCommand::Replace {
            state: Box::new(state),
        }],
    )?;
    log::info!("disposition restaurée ({} boîtes)", snapshot.boxes.len());
    Ok(())
}

/// Raccourcis des dispositions nommées (lecture seule).
pub fn hotkeys(store: &Store) -> Vec<(Uuid, String)> {
    peek_index(store).hotkeys()
}

/// Dispositions nommées pour le menu de la zone de notification.
pub fn names(store: &Store) -> Vec<(Uuid, String)> {
    peek_index(store)
        .layouts
        .into_iter()
        .map(|l| (l.id, l.name))
        .collect()
}

/// Lance une action hors du thread principal (raccourci, menu) ; les erreurs vont au
/// journal.
pub fn run_in_background(store: &Store, bus: &EventBus, action: &'static str, data: Value) {
    let (store, bus) = (store.clone(), bus.clone());
    std::thread::spawn(move || {
        if let Response::Error { message } = handle(&store, &bus, action, data) {
            log::warn!("dispositions : {message}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidy_core::DeskBox;
    use tidy_core::layouts::peek_index;

    /// Prendre, renommer, restaurer et appliquer, bureau arrêté (l'agent écrit
    /// lui-même `desktop.json`), sur des données jetables.
    #[test]
    fn take_restore_and_apply_without_desktop_process() {
        let data = tempfile::tempdir().unwrap();
        let store = Store::open(data.path()).unwrap();
        let bus = EventBus::default();
        let events = bus.subscribe();
        let mut first = DesktopState::default();
        first.boxes.push(DeskBox::new("Travail"));
        store.save(&first).unwrap();

        assert_eq!(
            handle(&store, &bus, "take", json!({ "name": "Avant" })),
            Response::Ok
        );
        let snap = peek_index(&store).snapshots[0].clone();
        assert_eq!(snap.kind, SnapshotKind::Manual);
        assert!(events.try_iter().any(|e| matches!(
            e,
            Event::Module { ref module, ref action, .. } if module == MODULE && action == "changed"
        )));

        let Response::Data { data: saved } =
            handle(&store, &bus, "save_layout", json!({ "name": "Bureau" }))
        else {
            panic!("identifiant attendu")
        };
        let layout_id = saved["id"].as_str().unwrap().to_owned();

        let mut second = first.clone();
        second.boxes.push(DeskBox::new("Jeux"));
        store.save(&second).unwrap();

        // Restaurer : instantané de sécurité, puis l'état de l'instantané.
        assert_eq!(
            handle(&store, &bus, "restore", json!({ "id": snap.id })),
            Response::Ok
        );
        assert_eq!(store.load::<DesktopState>().boxes, first.boxes);
        let index = peek_index(&store);
        assert_eq!(index.snapshots.len(), 2);
        assert_eq!(index.snapshots[1].reason, SnapshotReason::BeforeRestore);
        assert_eq!(index.snapshots[1].hash, fingerprint(&second));

        // Revenir à la précédente : l'état d'avant la restauration.
        assert_eq!(
            handle(&store, &bus, "restore_previous", Value::Null),
            Response::Ok
        );
        assert_eq!(store.load::<DesktopState>().boxes, second.boxes);

        assert_eq!(
            handle(&store, &bus, "apply_layout", json!({ "id": layout_id })),
            Response::Ok
        );
        assert_eq!(store.load::<DesktopState>().boxes, first.boxes);

        assert_eq!(
            handle(
                &store,
                &bus,
                "set_layout_hotkey",
                json!({ "id": layout_id, "hotkey": "control+alt+Digit1" })
            ),
            Response::Ok
        );
        assert_eq!(hotkeys(&store).len(), 1);
        assert!(matches!(
            handle(&store, &bus, "restore", json!({ "id": "pas un id" })),
            Response::Error { .. }
        ));
        assert!(matches!(
            handle(&store, &bus, "inconnue", Value::Null),
            Response::Error { .. }
        ));
    }
}
