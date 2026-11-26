//! Widgets (lots H et I) : réglages communs à tous les widgets.
//!
//! Fichier propre à son lot : les champs s'ajoutent ici, chacun avec une valeur par
//! défaut (`#[serde(default)]` sur la structure).
//!
//! Les réglages propres à un widget (fuseaux d'une horloge, durées d'un pomodoro…)
//! vivent dans son instance (`WidgetInstance::settings`), pas ici.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WidgetSettings {
    /// Les widgets se masquent avec le bureau (double-clic, raccourci), sauf ceux qui
    /// en sont exclus un par un.
    pub hide_with_desktop: bool,
    /// Son des notifications des widgets (fin d'un pomodoro, rappel).
    pub notification_sound: bool,
    /// Cadre commun (poignée, boutons) seulement au survol ; sinon toujours visible.
    pub frame_on_hover: bool,
}

impl Default for WidgetSettings {
    fn default() -> Self {
        Self {
            hide_with_desktop: true,
            notification_sound: true,
            frame_on_hover: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_defaults() {
        let s = WidgetSettings {
            hide_with_desktop: false,
            notification_sound: false,
            frame_on_hover: false,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<WidgetSettings>(&json).unwrap(), s);
        // Ancien fichier sans la section : valeurs par défaut.
        let old: WidgetSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(old, WidgetSettings::default());
        assert!(old.hide_with_desktop && old.notification_sound);
    }
}
