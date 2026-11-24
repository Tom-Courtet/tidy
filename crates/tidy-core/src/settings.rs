//! Réglages de l'application (`config.json`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::category::Category;
use crate::model::{Appearance, BoxId};
use crate::store::Versioned;

/// Langue de l'interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    /// Langue du système si elle est disponible, sinon le français.
    #[default]
    System,
    Fr,
    En,
}

/// Façon dont Tidy occupe le bureau (voir ADR 003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopMode {
    /// Boîtes désactivées : Tidy ne dessine rien sur le bureau.
    Off,
    /// Les boîtes flottent au-dessus des icônes du Finder, qui restent visibles.
    #[default]
    Overlay,
    /// Les icônes du Finder sont masquées ; Tidy dessine tout le bureau. Demande
    /// l'accord explicite de l'utilisateur ([`Settings::managed_consent`]).
    Managed,
}

/// Ce que masque le double-clic sur le bureau vide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuickHideScope {
    /// Boîtes, icônes et widgets.
    #[default]
    Everything,
    /// Seulement les boîtes.
    BoxesOnly,
}

/// Comportement du raccourci d'accès rapide aux boîtes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuickAccessMode {
    /// Les boîtes passent devant tant que la touche est enfoncée.
    #[default]
    Hold,
    /// Un appui les montre, le suivant les rend.
    Toggle,
}

/// Que faire des positions quand la résolution change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionPolicy {
    /// Une disposition par configuration d'écrans (défaut de l'original).
    #[default]
    RememberPerResolution,
    /// Garder la disposition en la mettant à l'échelle.
    Rescale,
    /// Ne rien faire.
    Nothing,
}

/// Raccourcis globaux (format de `global-hotkey`, ex. `"alt+Space"`). `None` : aucun.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkeys {
    pub search: Option<String>,
    pub toggle_desktop: Option<String>,
    pub quick_access: Option<String>,
    pub quick_access_mode: QuickAccessMode,
    pub main_window: Option<String>,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            search: Some("alt+Space".into()),
            toggle_desktop: Some("control+alt+KeyH".into()),
            quick_access: Some("alt+Backquote".into()),
            quick_access_mode: QuickAccessMode::Hold,
            main_window: None,
        }
    }
}

/// Actions rapides sur le bureau.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuickActions {
    pub double_click_to_hide: bool,
    pub hide_scope: QuickHideScope,
    /// Montrer les icônes au démarrage même si elles étaient masquées.
    pub show_on_start: bool,
    /// Créer une boîte en dessinant un rectangle sur le bureau vide.
    pub draw_to_create: bool,
    /// Ouvrir les éléments d'un simple clic (sinon double-clic).
    pub single_click_open: bool,
}

impl Default for QuickActions {
    fn default() -> Self {
        Self {
            double_click_to_hide: true,
            hide_scope: QuickHideScope::Everything,
            show_on_start: true,
            draw_to_create: true,
            single_click_open: false,
        }
    }
}

/// Réglage d'une catégorie dans l'écran « Rangement ».
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategorySetting {
    /// Boîte où vont les éléments de cette catégorie.
    pub target: Option<BoxId>,
    /// Extensions, si l'utilisateur a modifié la liste par défaut.
    #[serde(default)]
    pub extensions: Option<Vec<String>>,
    /// Ranger aussi les alias qui pointent vers ce type.
    #[serde(default)]
    pub include_aliases: bool,
}

/// Rangement automatique.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Organization {
    /// Ranger les nouveaux éléments dès leur arrivée.
    pub auto_apply_to_new: bool,
    /// Afficher dans la barre de titre le nombre d'éléments arrivés.
    pub show_new_count: bool,
    /// Boîte « fourre-tout » pour ce qu'aucune règle ne prend.
    pub catch_all: Option<BoxId>,
    pub categories: BTreeMap<Category, CategorySetting>,
}

impl Default for Organization {
    fn default() -> Self {
        Self {
            auto_apply_to_new: true,
            show_new_count: true,
            catch_all: None,
            categories: BTreeMap::new(),
        }
    }
}

/// Aimantation et disposition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutSettings {
    pub snap: bool,
    /// Écart entre boîtes aimantées, en points.
    pub spacing: f32,
    /// Taille des boîtes en multiples entiers d'icônes.
    pub size_in_icons: bool,
    /// Les boîtes évitent de recouvrir les icônes hors boîte.
    pub avoid_icons: bool,
    pub on_resolution_change: ResolutionPolicy,
    /// Enrouler les boîtes collées au bord de l'écran.
    pub roll_up_at_edge: bool,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            snap: true,
            spacing: 10.0,
            size_in_icons: false,
            avoid_icons: true,
            on_resolution_change: ResolutionPolicy::default(),
            roll_up_at_edge: false,
        }
    }
}

/// Instantanés de la disposition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnapshotSettings {
    pub auto: bool,
    /// Nombre d'instantanés automatiques gardés (le premier et les manuels ne
    /// comptent pas).
    pub keep: usize,
}

impl Default for SnapshotSettings {
    fn default() -> Self {
        Self {
            auto: true,
            keep: 50,
        }
    }
}

/// Réglages de Tidy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    pub language: Language,
    pub desktop_mode: DesktopMode,
    /// L'utilisateur a accepté que Tidy masque les icônes du Finder.
    pub managed_consent: bool,
    /// Assistant de premier lancement terminé.
    pub onboarding_done: bool,
    pub launch_at_login: bool,
    pub hotkeys: Hotkeys,
    pub quick_actions: QuickActions,
    pub organization: Organization,
    pub layout: LayoutSettings,
    pub snapshots: SnapshotSettings,
    /// Apparence des boîtes sans apparence propre.
    pub default_appearance: Appearance,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: Self::CURRENT_VERSION,
            language: Language::default(),
            desktop_mode: DesktopMode::default(),
            managed_consent: false,
            onboarding_done: false,
            launch_at_login: false,
            hotkeys: Hotkeys::default(),
            quick_actions: QuickActions::default(),
            organization: Organization::default(),
            layout: LayoutSettings::default(),
            snapshots: SnapshotSettings::default(),
            default_appearance: Appearance::default(),
        }
    }
}

impl Versioned for Settings {
    const CURRENT_VERSION: u32 = 1;
    const FILE_NAME: &'static str = "config.json";
}

impl Settings {
    /// Mode effectivement appliqué : le mode géré sans accord retombe en superposé.
    pub fn effective_desktop_mode(&self) -> DesktopMode {
        match self.desktop_mode {
            DesktopMode::Managed if !self.managed_consent => DesktopMode::Overlay,
            mode => mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_mode_needs_consent() {
        let mut s = Settings {
            desktop_mode: DesktopMode::Managed,
            ..Settings::default()
        };
        assert_eq!(s.effective_desktop_mode(), DesktopMode::Overlay);
        s.managed_consent = true;
        assert_eq!(s.effective_desktop_mode(), DesktopMode::Managed);
    }

    #[test]
    fn empty_json_gives_defaults() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.hotkeys, Hotkeys::default());
        assert!(s.quick_actions.double_click_to_hide);
        assert_eq!(s.snapshots.keep, 50);
    }
}
