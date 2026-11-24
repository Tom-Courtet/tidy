//! Raccourci clavier par boîte (`DeskBox::hotkey`) : fait passer la boîte devant les
//! applis quelques secondes.
//!
//! Enregistrés dans le processus du bureau, sur le thread principal une fois la boucle
//! lancée (exigence de macOS ; sous Windows, `global-hotkey` crée sa fenêtre de messages
//! sur ce thread). Un raccourci refusé (déjà pris, mal écrit) est noté dans le journal.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tidy_core::model::{BoxId, DeskBox};

/// Raccourcis voulus : une boîte, un texte de raccourci.
pub fn wanted(boxes: &[DeskBox]) -> Vec<(BoxId, String)> {
    boxes
        .iter()
        .filter_map(|b| {
            let text = b.hotkey.as_deref()?.trim();
            (!text.is_empty()).then(|| (b.id, text.to_owned()))
        })
        .collect()
}

pub struct BoxHotkeys {
    manager: Option<GlobalHotKeyManager>,
    /// Raccourcis enregistrés, par boîte (avec leur texte, pour voir les changements).
    registered: HashMap<BoxId, (String, HotKey)>,
    table: Arc<Mutex<HashMap<u32, BoxId>>>,
}

impl BoxHotkeys {
    /// `on_press` reçoit la boîte dont le raccourci est pressé ; `wake` réveille la
    /// boucle.
    pub fn new(on_press: Sender<BoxId>, wake: impl Fn() + Send + Sync + 'static) -> Self {
        let manager = GlobalHotKeyManager::new()
            .map_err(|e| log::warn!("raccourcis des boîtes indisponibles : {e}"))
            .ok();
        let table: Arc<Mutex<HashMap<u32, BoxId>>> = Arc::default();
        if manager.is_some() {
            let lookup = Arc::clone(&table);
            let on_press = Mutex::new(on_press);
            GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
                if event.state() != HotKeyState::Pressed {
                    return;
                }
                let id = lookup.lock().ok().and_then(|t| t.get(&event.id()).copied());
                if let (Some(id), Ok(tx)) = (id, on_press.lock()) {
                    let _ = tx.send(id);
                    wake();
                }
            }));
        }
        Self {
            manager,
            registered: HashMap::new(),
            table,
        }
    }

    /// Met les raccourcis enregistrés en accord avec les boîtes.
    pub fn sync(&mut self, boxes: &[DeskBox]) {
        let Some(manager) = &self.manager else {
            return;
        };
        let wanted: HashMap<BoxId, String> = wanted(boxes).into_iter().collect();
        // Retirer ceux qui ont changé ou disparu.
        self.registered.retain(|id, (text, hotkey)| {
            let keep = wanted.get(id) == Some(text);
            if !keep {
                let _ = manager.unregister(*hotkey);
            }
            keep
        });
        for (id, text) in wanted {
            if self.registered.contains_key(&id) {
                continue;
            }
            match HotKey::from_str(&text) {
                Ok(hotkey) => match manager.register(hotkey) {
                    Ok(()) => {
                        self.registered.insert(id, (text, hotkey));
                    }
                    Err(e) => log::warn!("raccourci de boîte « {text} » refusé : {e}"),
                },
                Err(e) => log::warn!("raccourci de boîte « {text} » illisible : {e}"),
            }
        }
        if let Ok(mut table) = self.table.lock() {
            *table = self
                .registered
                .iter()
                .map(|(id, (_, hotkey))| (hotkey.id(), *id))
                .collect();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_boxes_with_a_hotkey() {
        let mut a = DeskBox::new("A");
        a.hotkey = Some("alt+KeyB".into());
        let mut b = DeskBox::new("B");
        b.hotkey = Some("  ".into());
        let c = DeskBox::new("C");
        let w = wanted(&[a.clone(), b, c]);
        assert_eq!(w, [(a.id, "alt+KeyB".to_owned())]);
        assert!(HotKey::from_str(&w[0].1).is_ok());
    }
}
