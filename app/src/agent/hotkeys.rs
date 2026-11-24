//! Raccourcis globaux de l'agent.
//!
//! À créer sur le thread principal, une fois la boucle d'événements lancée (exigence
//! de macOS). Les raccourcis refusés (déjà pris par une autre application, ou mal
//! écrits) sont signalés dans le journal ; les autres restent actifs.

use std::collections::HashMap;
use std::str::FromStr;

use anyhow::{Context, Result};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tidy_core::settings::Hotkeys as HotkeySettings;

/// Action d'un raccourci global.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Search,
    ToggleDesktop,
    QuickAccess,
    MainWindow,
}

/// Raccourcis voulus, d'après les réglages.
pub fn wanted(settings: &HotkeySettings) -> Vec<(Action, &str)> {
    [
        (Action::Search, settings.search.as_deref()),
        (Action::ToggleDesktop, settings.toggle_desktop.as_deref()),
        (Action::QuickAccess, settings.quick_access.as_deref()),
        (Action::MainWindow, settings.main_window.as_deref()),
    ]
    .into_iter()
    .filter_map(|(action, text)| Some((action, text?)))
    .collect()
}

/// Garde les raccourcis enregistrés tant qu'elle vit.
pub struct Hotkeys {
    manager: GlobalHotKeyManager,
    registered: Vec<HotKey>,
}

impl Hotkeys {
    /// `on_action(action, appui)` est appelé à chaque appui (`true`) et relâchement.
    pub fn register(
        settings: &HotkeySettings,
        on_action: impl Fn(Action, bool) + Send + Sync + 'static,
    ) -> Result<Self> {
        let manager = GlobalHotKeyManager::new().context("raccourcis globaux indisponibles")?;
        let mut table = HashMap::new();
        let mut registered = Vec::new();
        for (action, text) in wanted(settings) {
            let hotkey = match HotKey::from_str(text) {
                Ok(h) => h,
                Err(e) => {
                    log::warn!("raccourci « {text} » illisible ({action:?}) : {e}");
                    continue;
                }
            };
            match manager.register(hotkey) {
                Ok(()) => {
                    table.insert(hotkey.id(), action);
                    registered.push(hotkey);
                }
                Err(e) => log::warn!("raccourci « {text} » refusé ({action:?}) : {e}"),
            }
        }
        log::info!("{} raccourcis globaux enregistrés", registered.len());
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if let Some(&action) = table.get(&event.id()) {
                on_action(action, event.state() == HotKeyState::Pressed);
            }
        }));
        Ok(Self {
            manager,
            registered,
        })
    }
}

impl Drop for Hotkeys {
    fn drop(&mut self) {
        let _ = self.manager.unregister_all(&self.registered);
        GlobalHotKeyEvent::set_event_handler(None::<fn(GlobalHotKeyEvent)>);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hotkeys_parse() {
        let settings = HotkeySettings::default();
        let wanted = wanted(&settings);
        assert_eq!(
            wanted.len(),
            3,
            "pas de raccourci par défaut pour la fenêtre"
        );
        for (action, text) in wanted {
            assert!(HotKey::from_str(text).is_ok(), "{action:?} : « {text} »");
        }
    }
}
