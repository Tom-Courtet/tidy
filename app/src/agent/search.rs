//! Recherche rapide côté agent (lot G) : le raccourci global et les demandes
//! `search/open` (widget « Search Mini », `tidy --search` lancé à la main) ouvrent le
//! lanceur (`tidy --search`), ou le ramènent s'il tourne déjà (fenêtre cachée,
//! réglage « Garder en mémoire »).
//!
//! L'agent ne garde que le processus du lanceur : l'index vit dans le lanceur, pas ici
//! (l'agent reste léger).

use std::process::{Child, Command};
use std::sync::Mutex;

use serde_json::{Value, json};
use tidy_core::events::{Event, EventBus};

use crate::ipc::Response;

/// Variable d'environnement posée sur le lanceur lancé par l'agent.
pub const CHILD_ENV: &str = "TIDY_SEARCH_CHILD";

/// Processus du lanceur, s'il a été lancé.
static LAUNCHER: Mutex<Option<Child>> = Mutex::new(None);

/// Demande `Request::Module { module: "search", … }`. `open` (données :
/// `{"query": "texte"}`) ouvre le lanceur ; les autres actions (`reindex`,
/// `recents_cleared`…) sont relayées au lanceur.
pub fn handle(bus: &EventBus, action: &str, data: Value) -> Response {
    match action {
        "open" => {
            let query = data
                .get("query")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .filter(|q| !q.trim().is_empty());
            open(bus, query);
        }
        _ => bus.publish(Event::Module {
            module: "search".into(),
            action: action.into(),
            data,
        }),
    }
    Response::Ok
}

/// Ouvre le lanceur, avec `query` déjà saisi, ou le ramène s'il tourne.
pub fn open(bus: &EventBus, query: Option<String>) {
    let Ok(mut launcher) = LAUNCHER.lock() else {
        return;
    };
    if let Some(child) = launcher.as_mut()
        && matches!(child.try_wait(), Ok(None))
    {
        bus.publish(Event::Module {
            module: "search".into(),
            action: "show".into(),
            data: json!({ "query": query }),
        });
        return;
    }
    let spawned = std::env::current_exe().and_then(|exe| {
        let mut cmd = Command::new(exe);
        cmd.arg("--search").env(CHILD_ENV, "1");
        if let Some(query) = &query {
            cmd.arg(query);
        }
        cmd.spawn()
    });
    match spawned {
        Ok(child) => {
            log::info!("lanceur de recherche ouvert (pid {})", child.id());
            *launcher = Some(child);
        }
        Err(e) => log::error!("lanceur de recherche impossible : {e}"),
    }
}
