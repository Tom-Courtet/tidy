//! Implémentation Windows (10 et 11), via `windows-sys`.
//!
//! - [`screens`] : écrans, identifiant stable, échelle par écran ;
//! - [`window`] : fenêtres posées sur le bureau (Win+D, ordre des fenêtres) ;
//! - [`desktop`] : icônes de l'Explorateur, double-clic et rectangle sur le bureau ;
//! - [`icons`] : positions des icônes (rangées = déplacées hors de l'écran) ;
//! - [`shell`] : icônes des fichiers, menu contextuel et glisser-déposer du Shell ;
//! - [`watch`] : changements d'écrans ;
//! - [`registry`] : menu contextuel du bureau, démarrage avec la session ;
//! - [`session`] : verrouillage de la session, veille, inactivité (lot F).

mod desktop;
mod icons;
mod registry;
mod screens;
mod session;
mod shell;
mod watch;
mod window;

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use raw_window_handle::HasWindowHandle;
use tidy_core::geometry::{Point, Rect};
use tidy_core::screen::Screen;
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SW_SHOWNORMAL, SetForegroundWindow,
};
use windows_sys::core::BOOL;

use crate::{
    Callback, DesktopPlatform, IconImage, LoginState, RectCallback, SessionCallback, SurfaceLevel,
};

pub struct WindowsPlatform;

/// Texte UTF-16 terminé par un zéro, pour les API « W ».
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Texte d'un tampon UTF-16 terminé par un zéro.
pub(crate) fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

impl DesktopPlatform for WindowsPlatform {
    fn screens(&self) -> Vec<Screen> {
        screens::monitors().into_iter().map(|m| m.screen).collect()
    }

    fn mouse_location(&self) -> Point {
        screens::mouse_location()
    }

    fn make_desktop_surface(&self, window: &dyn HasWindowHandle, frame: Rect, level: SurfaceLevel) {
        match window::hwnd(window) {
            Some(hwnd) => window::make_surface(hwnd, frame, level),
            None => log::warn!("surface du bureau : fenêtre native introuvable"),
        }
    }

    fn make_desktop_surface_titled(&self, title: &str, frame: Rect, level: SurfaceLevel) -> bool {
        match window::find_titled(title) {
            Some(hwnd) => {
                window::make_surface(hwnd, frame, level);
                true
            }
            None => false,
        }
    }

    fn desktop_icons_hidden(&self) -> bool {
        desktop::icons_hidden()
    }

    fn set_desktop_icons_hidden(&self, hidden: bool) -> Result<()> {
        desktop::set_icons_hidden(hidden)
    }

    fn watch_desktop_double_click(&self, on_double_click: Callback) -> bool {
        desktop::watch_double_click(on_double_click)
    }

    fn set_desktop_menu(&self, label: Option<&str>) -> Result<()> {
        registry::set_desktop_menu(label)
    }

    fn desktop_icon_positions(&self) -> Option<&'static dyn tidy_core::icon_stash::DesktopIcons> {
        Some(&icons::ExplorerIcons)
    }

    fn file_icon(&self, path: &Path, size_px: u32, thumbnail: bool) -> Option<IconImage> {
        shell::file_icon(path, size_px, thumbnail)
            .inspect_err(|e| log::debug!("icône de {} : {e:#}", path.display()))
            .ok()
    }

    fn show_file_menu(&self, window_title: &str, paths: &[PathBuf]) -> Result<bool> {
        shell::show_menu(window_title, paths)
    }

    fn start_file_drag(&self, window_title: &str, paths: &[PathBuf]) -> Result<bool> {
        shell::start_drag(window_title, paths)
    }

    fn trash(&self, paths: &[PathBuf]) -> Result<()> {
        crate::recycle::move_to_trash(paths)
    }

    fn watch_screens(&self, on_change: Callback) -> bool {
        watch::watch_screens(on_change)
    }

    fn watch_desktop_drag(&self, on_rect: RectCallback) -> bool {
        desktop::watch_drag(on_rect)
    }

    fn raise_desktop_surface(&self, title: &str, raised: bool) {
        match window::find_titled(title) {
            Some(hwnd) => window::raise(hwnd, raised),
            None => log::debug!("accès rapide : fenêtre « {title} » introuvable"),
        }
    }

    fn open(&self, path: &Path) -> Result<()> {
        let verb = wide("open");
        let file = wide(&path.to_string_lossy());
        // SAFETY: chaînes terminées par un zéro, vivantes pendant l'appel ; pas de
        // fenêtre parente.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        // Valeur > 32 : réussite (convention de ShellExecute).
        if result as isize <= 32 {
            bail!(
                "ouverture de {} impossible (code {})",
                path.display(),
                result as isize
            );
        }
        Ok(())
    }

    fn reveal(&self, path: &Path) -> Result<()> {
        // `explorer /select,"chemin"` : l'Explorateur lit lui-même ses arguments, sans
        // les règles de guillemets habituelles, d'où `raw_arg`.
        std::process::Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()?;
        Ok(())
    }

    fn preferred_languages(&self) -> Vec<String> {
        let mut buf = [0u16; 85];
        // SAFETY: tampon local, sa taille est passée à l'API (LOCALE_NAME_MAX_LENGTH).
        let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
        if n > 0 {
            vec![from_wide(&buf)]
        } else {
            Vec::new()
        }
    }

    fn login_item(&self) -> LoginState {
        registry::login_state()
    }

    fn set_login_item(&self, enabled: bool) -> Result<LoginState> {
        registry::set_login(enabled)
    }

    // --- Lot F (boîte privée). ---

    fn watch_session(&self, on_event: SessionCallback) -> bool {
        session::watch_session(on_event)
    }

    fn idle_seconds(&self) -> Option<f64> {
        session::idle_seconds()
    }
}

/// Ramène devant une fenêtre visible du processus `pid`. Windows peut le refuser si
/// l'agent n'a pas reçu la dernière entrée de l'utilisateur.
pub fn activate_process(pid: u32) -> bool {
    struct Search {
        pid: u32,
        found: HWND,
    }
    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
        // SAFETY: `data` est le `&mut Search` passé à EnumWindows ci-dessous.
        let search = unsafe { &mut *(data as *mut Search) };
        let mut owner = 0u32;
        // SAFETY: `hwnd` vient d'EnumWindows ; `owner` est une variable locale.
        unsafe { GetWindowThreadProcessId(hwnd, &mut owner) };
        // SAFETY: simple lecture d'état d'une fenêtre existante.
        if owner == search.pid && unsafe { IsWindowVisible(hwnd) } != 0 {
            search.found = hwnd;
            return 0;
        }
        1
    }
    let mut search = Search {
        pid,
        found: std::ptr::null_mut(),
    };
    // SAFETY: le rappel ne vit que pendant l'appel, comme `search`.
    unsafe { EnumWindows(Some(visit), &mut search as *mut Search as LPARAM) };
    if search.found.is_null() {
        return false;
    }
    // SAFETY: fenêtre trouvée ci-dessus, toujours vivante (au pire, l'appel échoue).
    unsafe { SetForegroundWindow(search.found) != 0 }
}
