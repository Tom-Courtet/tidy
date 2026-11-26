//! Fonds d'écran (lot J) : choix par écran, ajustement, playlist, volume, pause
//! intelligente, sources en ligne.
//!
//! Fichier propre à son lot : les champs s'ajoutent ici, chacun avec une valeur par
//! défaut (`#[serde(default)]` sur la structure).
//!
//! La bibliothèque elle-même (fichiers importés, catégories, favoris) vit dans
//! `wallpapers.json` (crate `tidy-wallpaper`) ; les réglages n'y font référence que
//! par identifiant.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::store::Versioned;

/// Ajustement d'une image ou d'une vidéo à l'écran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// Remplir l'écran en gardant les proportions (les bords dépassent).
    #[default]
    Fill,
    /// Tout montrer en gardant les proportions (bandes de la couleur de fond).
    Fit,
    /// Étirer sur tout l'écran (proportions perdues).
    Stretch,
    /// Taille réelle, au centre.
    Center,
    /// Mosaïque (images seulement ; une vidéo est alors ajustée).
    Tile,
}

impl Fit {
    pub const ALL: [Fit; 5] = [Fit::Fill, Fit::Fit, Fit::Stretch, Fit::Center, Fit::Tile];
}

/// Ce qu'affiche un écran.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Choice {
    /// Tidy ne touche pas à cet écran.
    #[default]
    None,
    /// Un fond de la bibliothèque (image ou vidéo).
    Item { id: Uuid },
    /// Couleur unie.
    Color { rgb: [u8; 3] },
    /// Le fond courant de la playlist.
    Playlist,
    /// Dernière image d'une source en ligne (`"bing"`), si les sources sont activées.
    Online { source: String },
}

/// Choix pour un écran donné (identifiant de `tidy_core::screen::ScreenId`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenChoice {
    pub screen: String,
    pub choice: Choice,
}

/// Ordre de la playlist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistOrder {
    #[default]
    InOrder,
    /// Aléatoire, sans répéter un fond avant d'avoir montré tous les autres.
    Shuffle,
}

/// Bornes de l'intervalle de la playlist, en minutes (de 5 min à 24 h, comme l'original).
pub const PLAYLIST_MIN_MINUTES: u32 = 5;
pub const PLAYLIST_MAX_MINUTES: u32 = 24 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaylistSettings {
    /// Fonds de la bibliothèque, dans l'ordre (images et vidéos mêlées).
    pub items: Vec<Uuid>,
    pub order: PlaylistOrder,
    /// Durée de chaque fond, bornée à [`PLAYLIST_MIN_MINUTES`]..=[`PLAYLIST_MAX_MINUTES`].
    pub interval_minutes: u32,
}

impl Default for PlaylistSettings {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            order: PlaylistOrder::default(),
            interval_minutes: 30,
        }
    }
}

impl PlaylistSettings {
    /// Intervalle borné, en minutes.
    pub fn interval(&self) -> u32 {
        self.interval_minutes
            .clamp(PLAYLIST_MIN_MINUTES, PLAYLIST_MAX_MINUTES)
    }
}

/// Pause intelligente : la vidéo s'arrête (et la playlist n'avance plus) quand…
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PauseSettings {
    /// … une appli est en plein écran ;
    pub on_fullscreen: bool,
    /// … l'ordinateur est sur batterie, au-dessous de `battery_threshold` % (100 :
    /// toujours sur batterie) ;
    pub on_battery: bool,
    pub battery_threshold: u8,
    /// … la session est verrouillée ;
    pub when_locked: bool,
    /// … le bureau est masqué (double-clic, raccourci).
    pub when_desktop_hidden: bool,
}

impl Default for PauseSettings {
    fn default() -> Self {
        Self {
            on_fullscreen: true,
            on_battery: true,
            battery_threshold: 100,
            when_locked: true,
            when_desktop_hidden: true,
        }
    }
}

/// Sources en ligne : toutes désactivées par défaut (aucun accès au réseau sans
/// l'accord de l'utilisateur).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OnlineSettings {
    /// Image du jour de Bing.
    pub bing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WallpaperSettings {
    /// Tidy gère le fond d'écran. Le désactiver rend le fond d'origine.
    pub enabled: bool,
    /// Le même fond sur tous les écrans (`all`), sinon un par écran (`screens`).
    pub same_on_all: bool,
    pub all: Choice,
    pub screens: Vec<ScreenChoice>,
    pub fit: Fit,
    /// Couleur autour d'une image ajustée (et des vidéos ajustées).
    pub background: [u8; 3],
    pub playlist: PlaylistSettings,
    /// Volume des vidéos, de 0 à 100.
    pub volume: u8,
    /// Vidéos muettes (par défaut).
    pub muted: bool,
    pub pause: PauseSettings,
    pub online: OnlineSettings,
}

impl Default for WallpaperSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            same_on_all: true,
            all: Choice::None,
            screens: Vec::new(),
            fit: Fit::default(),
            background: [0, 0, 0],
            playlist: PlaylistSettings::default(),
            volume: 50,
            muted: true,
            pause: PauseSettings::default(),
            online: OnlineSettings::default(),
        }
    }
}

impl WallpaperSettings {
    /// Choix qui s'applique à l'écran `screen`. Un par écran : un écran sans choix
    /// propre garde celui de tous les écrans.
    pub fn choice_for(&self, screen: &str) -> &Choice {
        if self.same_on_all {
            return &self.all;
        }
        self.screens
            .iter()
            .find(|s| s.screen == screen)
            .map_or(&self.all, |s| &s.choice)
    }

    /// Donne `choice` à l'écran `screen` (ou à tous si `screen` est `None`, ce qui
    /// passe en « le même partout »). En passant en « un par écran », les autres écrans
    /// gardent ce qu'ils montraient (le choix de tous les écrans) : les choix par écran
    /// d'une fois précédente sont oubliés.
    pub fn set_choice(&mut self, screen: Option<&str>, choice: Choice) {
        let Some(screen) = screen else {
            self.same_on_all = true;
            self.all = choice;
            return;
        };
        if self.same_on_all {
            self.screens.clear();
        }
        self.same_on_all = false;
        match self.screens.iter_mut().find(|s| s.screen == screen) {
            Some(s) => s.choice = choice,
            None => self.screens.push(ScreenChoice {
                screen: screen.to_owned(),
                choice,
            }),
        }
    }

    /// Un écran au moins affiche la playlist ? (Un par écran : les écrans sans choix
    /// propre comptent avec le choix de tous les écrans.)
    pub fn uses_playlist(&self) -> bool {
        self.all == Choice::Playlist
            || !self.same_on_all && self.screens.iter().any(|s| s.choice == Choice::Playlist)
    }

    /// Oublie un fond retiré de la bibliothèque (choix et playlist).
    pub fn forget_item(&mut self, id: Uuid) {
        let gone = |c: &Choice| matches!(c, Choice::Item { id: x } if *x == id);
        if gone(&self.all) {
            self.all = Choice::None;
        }
        for s in &mut self.screens {
            if gone(&s.choice) {
                s.choice = Choice::None;
            }
        }
        self.playlist.items.retain(|x| *x != id);
    }
}

/// Fond d'écran du système **avant** le premier changement fait par Tidy
/// (`wallpaper-original.json`), rendu quand on désactive les fonds de Tidy ou qu'on
/// le désinstalle. Écrit une seule fois ; effacé une fois rendu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OriginalWallpaper {
    pub schema_version: u32,
    pub screens: Vec<OriginalScreen>,
    /// Ajustement commun (Windows : un seul pour tous les écrans).
    pub fit: Option<Fit>,
    /// Couleur de fond commune (Windows).
    pub background: Option<[u8; 3]>,
}

impl Default for OriginalWallpaper {
    fn default() -> Self {
        Self {
            schema_version: Self::CURRENT_VERSION,
            screens: Vec::new(),
            fit: None,
            background: None,
        }
    }
}

impl Versioned for OriginalWallpaper {
    const CURRENT_VERSION: u32 = 1;
    const FILE_NAME: &'static str = "wallpaper-original.json";
}

/// Fond d'origine d'un écran. `screen` : identifiant propre au système (chemin du
/// moniteur sous Windows, identifiant de l'écran sous macOS).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OriginalScreen {
    pub screen: String,
    /// Image (absente : couleur unie).
    #[serde(default)]
    pub image: Option<PathBuf>,
    #[serde(default)]
    pub fit: Option<Fit>,
    #[serde(default)]
    pub background: Option<[u8; 3]>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_per_screen_or_everywhere() {
        let mut s = WallpaperSettings::default();
        assert_eq!(s.choice_for("a"), &Choice::None);
        let id = Uuid::new_v4();
        s.set_choice(Some("a"), Choice::Item { id });
        assert!(!s.same_on_all);
        assert_eq!(s.choice_for("a"), &Choice::Item { id });
        assert_eq!(s.choice_for("b"), &Choice::None);
        s.set_choice(None, Choice::Playlist);
        assert_eq!(s.choice_for("b"), &Choice::Playlist);
        assert!(s.uses_playlist());
    }

    /// Passer en « un par écran » ne vide pas les autres écrans.
    #[test]
    fn other_screens_keep_their_wallpaper_when_going_per_screen() {
        let mut s = WallpaperSettings::default();
        let (x, y) = (Uuid::new_v4(), Uuid::new_v4());
        // Choix par écran d'une fois précédente : oubliés au retour en « un par écran ».
        s.set_choice(Some("b"), Choice::Color { rgb: [1, 2, 3] });
        s.set_choice(None, Choice::Item { id: x });
        s.set_choice(Some("a"), Choice::Item { id: y });
        assert_eq!(s.choice_for("a"), &Choice::Item { id: y });
        assert_eq!(s.choice_for("b"), &Choice::Item { id: x });
        assert_eq!(s.choice_for("c"), &Choice::Item { id: x });
        // « Vider » un écran reste possible.
        s.set_choice(Some("b"), Choice::None);
        assert_eq!(s.choice_for("b"), &Choice::None);
    }

    #[test]
    fn forgetting_an_item_clears_its_uses() {
        let mut s = WallpaperSettings::default();
        let id = Uuid::new_v4();
        s.set_choice(Some("a"), Choice::Item { id });
        s.playlist.items = vec![id, Uuid::new_v4()];
        s.forget_item(id);
        assert_eq!(s.choice_for("a"), &Choice::None);
        assert_eq!(s.playlist.items.len(), 1);
    }

    #[test]
    fn playlist_interval_is_bounded() {
        let mut p = PlaylistSettings {
            interval_minutes: 1,
            ..Default::default()
        };
        assert_eq!(p.interval(), PLAYLIST_MIN_MINUTES);
        p.interval_minutes = 10_000;
        assert_eq!(p.interval(), PLAYLIST_MAX_MINUTES);
    }

    #[test]
    fn old_config_without_wallpaper_section_loads() {
        let s: crate::Settings = serde_json::from_str(r#"{"schema_version":1}"#).unwrap();
        assert_eq!(s.wallpaper, WallpaperSettings::default());
        assert!(s.wallpaper.muted, "muet par défaut");
        assert!(
            !s.wallpaper.online.bing,
            "sources en ligne désactivées par défaut"
        );
    }
}
