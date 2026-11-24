//! Événements qui circulent entre les modules (bus d'événements).
//!
//! Dans un processus, [`EventBus`] les distribue à des abonnés (canaux). Entre
//! processus, l'agent les pousse aux abonnés du socket (même type, sérialisé en JSON).

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::model::BoxId;
use crate::screen::Screen;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// Un élément est apparu dans `~/Desktop` (ou dans un dossier surveillé).
    FileCreated {
        path: PathBuf,
    },
    FileRemoved {
        path: PathBuf,
    },
    FileRenamed {
        from: PathBuf,
        to: PathBuf,
    },
    FileModified {
        path: PathBuf,
    },
    /// Les écrans ont changé (branchement, résolution, disposition).
    ScreensChanged {
        screens: Vec<Screen>,
    },
    /// `config.json` a changé : relire les réglages.
    SettingsChanged,
    /// `desktop.json` a changé : relire l'état du bureau.
    DesktopChanged,
    /// Une boîte a changé (position, contenu, apparence).
    BoxChanged {
        id: BoxId,
    },
    /// Masquer (`hidden: true`) ou montrer le bureau.
    DesktopVisibility {
        hidden: bool,
    },
    /// Basculer la visibilité du bureau (raccourci, double-clic).
    ToggleDesktop,
    /// Accès rapide aux boîtes : `active` pendant l'appui (ou jusqu'au suivant).
    QuickAccess {
        active: bool,
    },
    /// Ouvrir le lanceur de recherche.
    OpenSearch,
    /// Ramener la fenêtre principale devant (elle est déjà ouverte).
    FocusManager,
    /// Créer une boîte (menu du bureau, `tidy --create-box`), à `at` ou sous la souris.
    CreateBox {
        at: Option<crate::geometry::Point>,
    },
    /// L'agent s'arrête : les autres processus se ferment.
    Shutdown,
}

/// Distributeur d'événements dans un processus. Clonable ; les abonnés disparus sont
/// oubliés au premier envoi qui échoue.
#[derive(Clone, Default)]
pub struct EventBus {
    subscribers: Arc<Mutex<Vec<Sender<Event>>>>,
}

impl EventBus {
    pub fn subscribe(&self) -> Receiver<Event> {
        let (tx, rx) = mpsc::channel();
        self.lock().push(tx);
        rx
    }

    pub fn publish(&self, event: Event) {
        self.lock().retain(|tx| tx.send(event.clone()).is_ok());
    }

    pub fn subscriber_count(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Sender<Event>>> {
        // Un abonné qui a paniqué ne doit pas bloquer les autres.
        self.subscribers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_subscriber_receives_and_dropped_ones_are_forgotten() {
        let bus = EventBus::default();
        let a = bus.subscribe();
        let b = bus.subscribe();
        bus.publish(Event::SettingsChanged);
        assert_eq!(a.try_recv().unwrap(), Event::SettingsChanged);
        assert_eq!(b.try_recv().unwrap(), Event::SettingsChanged);
        drop(b);
        bus.publish(Event::ToggleDesktop);
        assert_eq!(bus.subscriber_count(), 1);
        assert_eq!(a.try_recv().unwrap(), Event::ToggleDesktop);
    }

    #[test]
    fn events_serialize_with_a_type_tag() {
        let json = serde_json::to_string(&Event::DesktopVisibility { hidden: true }).unwrap();
        assert_eq!(json, r#"{"type":"desktop_visibility","hidden":true}"#);
    }
}
