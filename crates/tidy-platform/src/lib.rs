//! Couche système de Tidy : tout ce qui touche à l'OS passe par ici.
//!
//! Le reste de l'application ne voit que le trait [`DesktopPlatform`] (renvoyé par
//! [`current`]) et des types de `tidy-core`. macOS est implémenté ; Windows compile,
//! avec des implémentations vides qui renvoient une erreur claire (portage prévu après
//! la parité, voir ADR 001).
//!
//! C'est la seule crate du dépôt où `unsafe` est permis : chaque bloc porte un
//! commentaire `// SAFETY:`.

use std::path::Path;

use anyhow::Result;
use raw_window_handle::HasWindowHandle;
use tidy_core::geometry::{Point, Rect};
use tidy_core::screen::Screen;

pub mod finder;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod other;

/// État du lancement à l'ouverture de session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginState {
    On,
    Off,
    /// Enregistré, mais à autoriser par l'utilisateur dans les réglages du système.
    NeedsApproval,
    /// Impossible ici (exécutable lancé hors de `Tidy.app`, OS non pris en charge).
    Unavailable(String),
}

/// Niveau d'une fenêtre posée sur le bureau.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceLevel {
    /// Juste au-dessus des icônes du bureau, sous toutes les fenêtres des applis :
    /// boîtes, widgets, icônes dessinées par Tidy.
    DesktopIcons,
    /// Au niveau du fond d'écran (fonds animés).
    Wallpaper,
}

/// Services système dont Tidy a besoin.
pub trait DesktopPlatform: Send + Sync {
    /// Écrans branchés, en points, coordonnées globales (origine en haut à gauche de
    /// l'écran principal).
    fn screens(&self) -> Vec<Screen>;

    /// Position de la souris, mêmes coordonnées que [`DesktopPlatform::screens`].
    fn mouse_location(&self) -> Point;

    /// Prépare une fenêtre pour le bureau : sans bordure ni ombre, au niveau `level`,
    /// sur tous les Spaces, immobile pendant Mission Control et « Afficher le
    /// bureau », absente de ⌘Tab, placée exactement sur `frame`.
    fn make_desktop_surface(&self, window: &dyn HasWindowHandle, frame: Rect, level: SurfaceLevel);

    /// Même chose pour une fenêtre secondaire d'egui, retrouvée par son titre. `false`
    /// si elle n'existe pas encore (à réessayer à l'image suivante).
    fn make_desktop_surface_titled(&self, title: &str, frame: Rect, level: SurfaceLevel) -> bool;

    /// Mode superposé : la surface de titre `title` ne reçoit les clics que dans
    /// `regions` (coordonnées globales) ; ailleurs, ils passent aux icônes du Finder.
    /// Liste vide : la surface entière laisse passer les clics. `None` : la surface
    /// entière reçoit les clics (mode géré).
    fn set_interactive_regions(&self, title: &str, regions: Option<Vec<Rect>>);

    /// Ouvre un fichier ou un dossier avec l'application par défaut.
    fn open(&self, path: &Path) -> Result<()>;

    /// Montre l'élément dans le Finder (ou l'Explorateur).
    fn reveal(&self, path: &Path) -> Result<()>;

    /// Codes des langues préférées de l'utilisateur, dans l'ordre (`fr-FR`, `en`…).
    fn preferred_languages(&self) -> Vec<String>;

    fn login_item(&self) -> LoginState;
    fn set_login_item(&self, enabled: bool) -> Result<LoginState>;
}

/// macOS : appelle `on_reopen` quand l'utilisateur rouvre Tidy.app alors qu'il tourne
/// (Finder, Launchpad, Spotlight). À installer une fois, sur le thread principal, la
/// boucle d'événements lancée. Sans effet ailleurs.
pub fn on_reopen(on_reopen: impl Fn() + Send + Sync + 'static) {
    #[cfg(target_os = "macos")]
    macos::reopen::install(on_reopen);
    #[cfg(not(target_os = "macos"))]
    let _ = on_reopen;
}

/// Implémentation pour l'OS courant.
pub fn current() -> &'static dyn DesktopPlatform {
    #[cfg(target_os = "macos")]
    {
        &macos::MacPlatform
    }
    #[cfg(not(target_os = "macos"))]
    {
        &other::OtherPlatform
    }
}
