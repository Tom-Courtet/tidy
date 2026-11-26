//! Couche système de Tidy : tout ce qui touche à l'OS passe par ici.
//!
//! Le reste de l'application ne voit que le trait [`DesktopPlatform`] (renvoyé par
//! [`current`]) et des types de `tidy-core`. Windows (cible prioritaire, ADR 001) et
//! macOS sont implémentés ; les autres OS compilent avec des réponses vides.
//!
//! C'est la seule crate du dépôt où `unsafe` est permis : chaque bloc porte un
//! commentaire `// SAFETY:`.
//!
//! Coordonnées : en **points**, origine en haut à gauche de l'écran principal. Sous
//! Windows, un point vaut un pixel physique divisé par l'échelle de *son* écran (voir
//! `windows/screens.rs`) : avec des écrans d'échelles différentes, les cadres logiques
//! peuvent laisser des trous ou se chevaucher, mais toute position est rangée par écran
//! et reconvertie avec l'échelle de cet écran.

use std::path::Path;

use anyhow::Result;
use raw_window_handle::HasWindowHandle;
use tidy_core::geometry::{Point, Rect};
use tidy_core::screen::Screen;

mod convert;
pub mod finder;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "macos", windows)))]
mod other;
#[cfg(any(target_os = "macos", windows))]
mod recycle;
#[cfg(windows)]
mod windows;

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
    /// boîtes, widgets.
    DesktopIcons,
    /// Au niveau du fond d'écran (fonds animés).
    Wallpaper,
}

/// Rappel appelé depuis le thread système (double-clic sur le bureau…).
pub type Callback = Box<dyn Fn() + Send + Sync>;

/// Rappel recevant un rectangle (coordonnées globales en points).
pub type RectCallback = Box<dyn Fn(Rect) + Send + Sync>;

/// Image RGBA (icône système, miniature).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconImage {
    pub width: u32,
    pub height: u32,
    /// Pixels RGBA, non prémultipliés, ligne par ligne.
    pub rgba: Vec<u8>,
}

/// Services système dont Tidy a besoin.
pub trait DesktopPlatform: Send + Sync {
    /// Écrans branchés (voir les coordonnées en tête de crate).
    fn screens(&self) -> Vec<Screen>;

    /// Position de la souris, mêmes coordonnées que [`DesktopPlatform::screens`].
    fn mouse_location(&self) -> Point;

    /// Prépare une fenêtre pour le bureau : sans bordure ni ombre, au niveau `level`,
    /// sur tous les bureaux virtuels, immobile pendant « Afficher le bureau » (Win+D,
    /// Mission Control), absente de la barre des tâches et du sélecteur de fenêtres,
    /// placée exactement sur `frame`.
    fn make_desktop_surface(&self, window: &dyn HasWindowHandle, frame: Rect, level: SurfaceLevel);

    /// Même chose pour une fenêtre secondaire d'egui, retrouvée par son titre. `false`
    /// si elle n'existe pas encore (à réessayer à l'image suivante).
    fn make_desktop_surface_titled(&self, title: &str, frame: Rect, level: SurfaceLevel) -> bool;

    /// Icônes du bureau du système masquées ? (Windows : celles de l'Explorateur.)
    fn desktop_icons_hidden(&self) -> bool;

    /// Masque ou montre toutes les icônes du bureau du système. Sans redémarrer
    /// l'Explorateur ni rien écrire sur le disque : un redémarrage de session les rend
    /// toujours. Erreur si ce n'est pas possible sur cet OS.
    fn set_desktop_icons_hidden(&self, hidden: bool) -> Result<()>;

    /// Surveille les double-clics sur une zone vide du bureau du système et appelle
    /// `on_double_click`. À appeler une fois, sur le thread principal (boucle de
    /// messages). `false` si ce n'est pas disponible sur cet OS.
    fn watch_desktop_double_click(&self, on_double_click: Callback) -> bool;

    /// Ajoute (`Some(libellé)`) ou retire (`None`) l'entrée « Créer une boîte » du menu
    /// contextuel du bureau. Elle lance `tidy --create-box`.
    fn set_desktop_menu(&self, label: Option<&str>) -> Result<()>;

    /// Positions des icônes du bureau du système, pour masquer les icônes rangées en
    /// les déplaçant hors de l'écran (Windows). `None` si ce n'est pas possible ici.
    fn desktop_icon_positions(&self) -> Option<&'static dyn tidy_core::icon_stash::DesktopIcons> {
        None
    }

    // --- Vague 1, lot B (plateforme). Valeurs par défaut : « pas disponible ». ---

    /// Icône système de l'élément (ou sa miniature si `thumbnail` et si le système en
    /// fournit une), à `size_px` pixels de côté.
    fn file_icon(&self, _path: &Path, _size_px: u32, _thumbnail: bool) -> Option<IconImage> {
        None
    }

    /// Menu contextuel natif de ces éléments (Explorateur / Finder), sous la souris,
    /// pour la fenêtre de titre `window_title`. `Ok(false)` : pas de menu natif ici,
    /// l'appelant affiche le sien.
    fn show_file_menu(&self, _window_title: &str, _paths: &[std::path::PathBuf]) -> Result<bool> {
        Ok(false)
    }

    /// Commence un glisser-déposer système de ces fichiers depuis la fenêtre
    /// `window_title` (vers l'Explorateur, le Finder, une autre appli). `Ok(false)` :
    /// pas disponible.
    fn start_file_drag(&self, _window_title: &str, _paths: &[std::path::PathBuf]) -> Result<bool> {
        Ok(false)
    }

    /// Met des fichiers à la corbeille (jamais de suppression définitive).
    fn trash(&self, _paths: &[std::path::PathBuf]) -> Result<()> {
        anyhow::bail!("corbeille indisponible")
    }

    /// Prévient quand les écrans changent (branchement, résolution, échelle). À
    /// appeler sur le thread principal. `false` : pas disponible.
    fn watch_screens(&self, _on_change: Callback) -> bool {
        false
    }

    /// Prévient quand l'utilisateur trace un rectangle de sélection sur une zone vide
    /// du bureau du système (pour proposer « Créer une boîte »). À appeler sur le thread
    /// principal. `false` : pas disponible.
    fn watch_desktop_drag(&self, _on_rect: RectCallback) -> bool {
        false
    }

    /// Accès rapide : passe la fenêtre `title` au-dessus des applis (`true`) ou la
    /// remet au niveau du bureau (`false`).
    fn raise_desktop_surface(&self, _title: &str, _raised: bool) {}

    /// Ouvre un fichier ou un dossier avec l'application par défaut.
    fn open(&self, path: &Path) -> Result<()>;

    /// Montre l'élément dans le Finder (ou l'Explorateur).
    fn reveal(&self, path: &Path) -> Result<()>;

    /// Codes des langues préférées de l'utilisateur, dans l'ordre (`fr-FR`, `en`…).
    fn preferred_languages(&self) -> Vec<String>;

    fn login_item(&self) -> LoginState;
    fn set_login_item(&self, enabled: bool) -> Result<LoginState>;

    // --- Vague 2, lot H (widgets). Valeur par défaut : « pas disponible ». ---

    /// Notification native (centre de notifications de Windows ou de macOS), avec ou
    /// sans son. Ne bloque pas : l'envoi se fait sur un fil à part. `false` : pas
    /// disponible sur cet OS.
    fn notify(&self, _title: &str, _body: &str, _sound: bool) -> bool {
        false
    }
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

/// Passe au premier plan les fenêtres d'un autre processus de Tidy (la fenêtre
/// principale déjà ouverte, par exemple). `false` si c'est impossible.
pub fn activate_process(pid: u32) -> bool {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        NSRunningApplication::runningApplicationWithProcessIdentifier(pid).is_some_and(|app| {
            app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows)
        })
    }
    #[cfg(windows)]
    {
        windows::activate_process(pid)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = pid;
        false
    }
}

/// Implémentation pour l'OS courant.
pub fn current() -> &'static dyn DesktopPlatform {
    #[cfg(target_os = "macos")]
    {
        &macos::MacPlatform
    }
    #[cfg(windows)]
    {
        &windows::WindowsPlatform
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        &other::OtherPlatform
    }
}
