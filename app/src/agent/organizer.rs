//! Rangement : règles, nouveaux fichiers du bureau, « Tout annuler ».
//!
//! **Lot A (vague 1)** : surveillance de `~/Desktop` (crate `notify`), rangement des
//! nouveaux éléments selon les règles, « Appliquer aux non affectés / à tous »,
//! instantané `desktop.before-organize.json` avant le premier rangement et annulation.
//! Toute modification de l'état passe par [`route`] (un seul écrivain : le bureau).

use tidy_core::events::{Event, EventBus};
use tidy_core::{DesktopCommand, DesktopState, Store};

use crate::ipc::Response;

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

/// Applique les règles (lot A).
pub fn apply_rules(_store: &Store, _bus: &EventBus, _all: bool) -> Response {
    Response::Error {
        message: "rangement automatique : à venir (vague 1, lot A)".into(),
    }
}

/// Annule le dernier rangement (lot A).
pub fn undo(_store: &Store, _bus: &EventBus) -> Response {
    Response::Error {
        message: "annulation du rangement : à venir (vague 1, lot A)".into(),
    }
}
