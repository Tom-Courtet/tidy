//! Demandes adressées aux modules des vagues 2 et 3 ([`Request::Module`]).
//!
//! Chaque lot ajoute **une ligne** à l'aiguillage de [`handle`] vers sa propre fonction
//! (dans son propre fichier de `agent/`). Un module sans traitement dans l'agent voit
//! sa demande relayée à tous les abonnés ([`Event::Module`]) : c'est ainsi que la
//! fenêtre principale ou un widget parle au processus du bureau, à la recherche ou au
//! fond d'écran.
//!
//! [`Request::Module`]: crate::ipc::Request::Module

use tidy_core::Store;
use tidy_core::events::{Event, EventBus};

use crate::ipc::Response;

/// Traite une demande de module.
pub fn handle(
    store: &Store,
    bus: &EventBus,
    module: String,
    action: String,
    data: serde_json::Value,
) -> Response {
    let _ = store;
    #[allow(clippy::match_single_binding)]
    match module.as_str() {
        // Une ligne par module traité par l'agent, par exemple :
        // "layouts" => super::layouts::handle(store, bus, &action, data),
        "layouts" => super::layouts::handle(store, bus, &action, data),
        _ => {
            bus.publish(Event::Module {
                module,
                action,
                data,
            });
            Response::Ok
        }
    }
}
