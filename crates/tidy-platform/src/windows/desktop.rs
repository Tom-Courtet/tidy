//! Bureau de l'Explorateur : ses icônes et le double-clic sur une zone vide.
//!
//! Les icônes du bureau sont une liste (`SysListView32`, nommée « FolderView ») dans
//! `SHELLDLL_DefView`, elle-même dans `Progman` ou, selon la version de Windows et le
//! fond d'écran, dans une fenêtre `WorkerW`. Masquer cette liste masque toutes les
//! icônes, sans rien écrire sur le disque ni injecter de code dans l'Explorateur ; un
//! redémarrage de l'Explorateur ou de la session les rend toujours.
//!
//! Double-clic : un crochet souris bas niveau (`WH_MOUSE_LL`, sans DLL) repère deux
//! clics rapprochés sur le bureau. Clic sur une icône ou sur le vide ? Un clic sur le
//! vide désélectionne tout : un court instant après, la liste n'a plus d'élément
//! sélectionné. C'est le seul cas qui déclenche le rappel.

use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Result, bail};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetParent,
    GetSystemMetrics, IsWindowVisible, MSLLHOOKSTRUCT, SM_CXDOUBLECLK, SM_CYDOUBLECLK,
    SMTO_ABORTIFHUNG, SW_HIDE, SW_SHOW, SendMessageTimeoutW, SetWindowsHookExW, ShowWindow,
    WH_MOUSE_LL, WM_LBUTTONDOWN, WindowFromPoint,
};
use windows_sys::core::BOOL;

use super::{from_wide, wide};
use crate::Callback;

/// `LVM_GETSELECTEDCOUNT` (commctrl.h).
const LVM_GETSELECTEDCOUNT: u32 = 0x1032;
/// Délai avant de lire la sélection : le temps que l'Explorateur traite le clic.
const SELECTION_DELAY: Duration = Duration::from_millis(80);

fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    // SAFETY: tampon local, taille passée à l'API ; `hwnd` peut être nul (échec sans
    // effet).
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 0 {
        return String::new();
    }
    from_wide(&buf[..n as usize])
}

fn child(parent: HWND, class: &str) -> HWND {
    let class = wide(class);
    // SAFETY: nom de classe terminé par un zéro ; parent nul = fenêtres de premier
    // niveau.
    unsafe {
        FindWindowExW(
            parent,
            std::ptr::null_mut(),
            class.as_ptr(),
            std::ptr::null(),
        )
    }
}

/// `SHELLDLL_DefView` du bureau : dans `Progman`, sinon dans une `WorkerW`.
fn def_view() -> HWND {
    let progman_class = wide("Progman");
    // SAFETY: nom de classe terminé par un zéro.
    let progman = unsafe { FindWindowW(progman_class.as_ptr(), std::ptr::null()) };
    let view = child(progman, "SHELLDLL_DefView");
    if !view.is_null() {
        return view;
    }
    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
        let class = wide("SHELLDLL_DefView");
        // SAFETY: nom terminé par un zéro ; `hwnd` vient d'EnumWindows.
        let view =
            unsafe { FindWindowExW(hwnd, std::ptr::null_mut(), class.as_ptr(), std::ptr::null()) };
        if view.is_null() {
            return 1;
        }
        // SAFETY: `data` est le `&mut HWND` passé à EnumWindows ci-dessous.
        unsafe { *(data as *mut HWND) = view };
        0
    }
    let mut found: HWND = std::ptr::null_mut();
    // SAFETY: le rappel ne vit que pendant l'appel, comme `found`.
    unsafe { EnumWindows(Some(visit), &mut found as *mut HWND as LPARAM) };
    found
}

/// Liste des icônes du bureau (`FolderView`).
fn folder_view() -> HWND {
    child(def_view(), "SysListView32")
}

pub fn icons_hidden() -> bool {
    let view = folder_view();
    // SAFETY: simple lecture de l'état d'une fenêtre (nulle : faux).
    !view.is_null() && unsafe { IsWindowVisible(view) } == 0
}

pub fn set_icons_hidden(hidden: bool) -> Result<()> {
    let view = folder_view();
    if view.is_null() {
        bail!("liste des icônes du bureau introuvable");
    }
    // SAFETY: fenêtre de l'Explorateur trouvée ci-dessus ; ShowWindow fonctionne d'un
    // processus à l'autre et ne fait que montrer ou cacher.
    unsafe { ShowWindow(view, if hidden { SW_HIDE } else { SW_SHOW }) };
    Ok(())
}

/// La fenêtre sous le clic fait-elle partie du bureau de l'Explorateur ?
fn is_desktop(hwnd: HWND) -> bool {
    match class_of(hwnd).as_str() {
        "SHELLDLL_DefView" | "Progman" | "WorkerW" => true,
        // SAFETY: lecture du parent d'une fenêtre existante.
        "SysListView32" => class_of(unsafe { GetParent(hwnd) }) == "SHELLDLL_DefView",
        _ => false,
    }
}

/// Aucune icône sélectionnée (ou icônes masquées) : le double-clic était sur le vide.
fn nothing_selected() -> bool {
    let view = folder_view();
    // SAFETY: lecture de l'état d'une fenêtre (nulle : faux).
    if view.is_null() || unsafe { IsWindowVisible(view) } == 0 {
        return true;
    }
    let mut count = 0usize;
    // SAFETY: message sans pointeur (aucune mémoire à partager entre processus) ; délai
    // court si l'Explorateur ne répond pas.
    let ok = unsafe {
        SendMessageTimeoutW(
            view,
            LVM_GETSELECTEDCOUNT,
            0,
            0,
            SMTO_ABORTIFHUNG,
            200,
            &mut count,
        )
    };
    ok != 0 && count == 0
}

static ON_DOUBLE_CLICK: OnceLock<Callback> = OnceLock::new();
/// Dernier clic sur le bureau : (heure en ms, position).
static LAST_CLICK: Mutex<Option<(u32, POINT)>> = Mutex::new(None);

/// Deux clics forment-ils un double-clic (délai et distance du système) ?
fn is_double(
    first: (u32, POINT),
    second: (u32, POINT),
    max_ms: u32,
    max_dx: i32,
    max_dy: i32,
) -> bool {
    second.0.wrapping_sub(first.0) <= max_ms
        && (second.1.x - first.1.x).abs() <= max_dx / 2
        && (second.1.y - first.1.y).abs() <= max_dy / 2
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && wparam as u32 == WM_LBUTTONDOWN && lparam != 0 {
        // SAFETY: pour WH_MOUSE_LL, `lparam` pointe sur une MSLLHOOKSTRUCT valide pendant
        // l'appel.
        let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        let click = (info.time, info.pt);
        // SAFETY: point reçu du système ; renvoie une fenêtre ou nul.
        let target = unsafe { WindowFromPoint(info.pt) };
        let mut last = LAST_CLICK.lock().unwrap_or_else(|p| p.into_inner());
        if is_desktop(target) {
            // SAFETY: fonctions sans argument ni effet de bord.
            let (max_ms, dx, dy) = unsafe {
                (
                    GetDoubleClickTime(),
                    GetSystemMetrics(SM_CXDOUBLECLK),
                    GetSystemMetrics(SM_CYDOUBLECLK),
                )
            };
            match *last {
                Some(first) if is_double(first, click, max_ms, dx, dy) => {
                    *last = None;
                    // Ne pas bloquer le crochet : la vérification se fait à part.
                    std::thread::spawn(|| {
                        std::thread::sleep(SELECTION_DELAY);
                        if nothing_selected()
                            && let Some(callback) = ON_DOUBLE_CLICK.get()
                        {
                            callback();
                        }
                    });
                }
                _ => *last = Some(click),
            }
        } else {
            *last = None;
        }
    }
    // SAFETY: on passe le relais au crochet suivant, avec les mêmes arguments.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

pub fn watch_double_click(on_double_click: Callback) -> bool {
    if ON_DOUBLE_CLICK.set(on_double_click).is_err() {
        return true;
    }
    // SAFETY: crochet global bas niveau, procédure dans cet exécutable ; il reste
    // installé jusqu'à la fin du processus. Le thread appelant pompe les messages (boucle
    // de l'agent), condition des crochets bas niveau.
    let hook = unsafe {
        SetWindowsHookExW(
            WH_MOUSE_LL,
            Some(mouse_hook),
            GetModuleHandleW(std::ptr::null()),
            0,
        )
    };
    if hook.is_null() {
        log::warn!("crochet souris refusé : pas de double-clic sur le bureau");
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_click_needs_time_and_distance() {
        let p = |x, y| POINT { x, y };
        assert!(is_double((1000, p(10, 10)), (1300, p(12, 11)), 500, 4, 4));
        assert!(!is_double((1000, p(10, 10)), (1600, p(10, 10)), 500, 4, 4));
        assert!(!is_double((1000, p(10, 10)), (1100, p(30, 10)), 500, 4, 4));
        // Compteur de millisecondes qui repasse par zéro.
        assert!(is_double(
            (u32::MAX - 100, p(0, 0)),
            (100, p(0, 0)),
            500,
            4,
            4
        ));
    }
}
