//! Fenêtres posées sur le bureau, sous Windows.
//!
//! Une boîte doit rester **sous toutes les applis** et **survivre à Win+D** (« Afficher
//! le bureau »), comme les boîtes de l'original. Deux réglages, cumulés :
//!
//! 1. **Propriétaire `Progman`** (la fenêtre du bureau) : une fenêtre possédée reste
//!    toujours au-dessus de son propriétaire, et « Afficher le bureau » ne masque pas
//!    les fenêtres possédées par le bureau.
//! 2. **Toujours en bas de la pile** : une sous-classe de la fenêtre remplace tout
//!    changement d'ordre (clic, activation) par `HWND_BOTTOM`. Avec le propriétaire, la
//!    fenêtre se retrouve juste au-dessus du bureau.
//!
//! Plus `WS_EX_TOOLWINDOW` : ni barre des tâches, ni Alt+Tab.
//!
//! Variante d'essai : `TIDY_WIN_DESKTOP=bottom` n'applique que le point 2 (sans
//! propriétaire), pour comparer sur une machine où le point 1 poserait problème.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tidy_core::geometry::Rect;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumThreadWindows, FindWindowW, GWL_EXSTYLE, GWLP_HWNDPARENT, GetWindowLongPtrW,
    GetWindowTextW, HWND_BOTTOM, IsWindowVisible, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW,
    SetWindowLongPtrW, SetWindowPos, WINDOWPOS, WM_WINDOWPOSCHANGING, WS_EX_APPWINDOW,
    WS_EX_TOOLWINDOW,
};
use windows_sys::core::BOOL;

use super::{screens, wide};
use crate::SurfaceLevel;

/// Identifiant de notre sous-classe (arbitraire, unique dans ce processus).
const SUBCLASS_ID: usize = 0x7469_6479; // « tidy »

pub fn hwnd(window: &dyn HasWindowHandle) -> Option<HWND> {
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as HWND),
        _ => None,
    }
}

/// Fenêtre de ce thread qui porte ce titre.
pub fn find_titled(title: &str) -> Option<HWND> {
    unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
        // SAFETY: `data` est le `&mut Vec` passé à EnumThreadWindows ci-dessous.
        let list = unsafe { &mut *(data as *mut Vec<HWND>) };
        list.push(hwnd);
        1
    }
    let mut windows: Vec<HWND> = Vec::new();
    // SAFETY: les fenêtres de winit vivent sur ce thread ; le rappel ne vit que pendant
    // l'appel, comme `windows`.
    unsafe {
        EnumThreadWindows(
            GetCurrentThreadId(),
            Some(collect),
            &mut windows as *mut Vec<HWND> as LPARAM,
        )
    };
    let wanted: Vec<u16> = title.encode_utf16().collect();
    // Seulement une fenêtre visible : une fenêtre fermée peut rester un instant.
    windows.into_iter().find(|&hwnd| {
        // SAFETY: simple lecture d'état d'une fenêtre de ce thread.
        if unsafe { IsWindowVisible(hwnd) } == 0 {
            return false;
        }
        let mut buf = [0u16; 256];
        // SAFETY: tampon local de 256 caractères, taille passée à l'API.
        let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        n > 0 && buf[..n as usize] == wanted[..]
    })
}

/// Fenêtre du bureau (`Progman`).
fn progman() -> HWND {
    let class = wide("Progman");
    // SAFETY: nom de classe terminé par un zéro.
    unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) }
}

fn owner_mode() -> bool {
    std::env::var("TIDY_WIN_DESKTOP").as_deref() != Ok("bottom")
}

/// Sous-classe : toute remontée dans la pile devient « tout en bas ».
unsafe extern "system" fn keep_at_bottom(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if msg == WM_WINDOWPOSCHANGING && lparam != 0 {
        // SAFETY: pour WM_WINDOWPOSCHANGING, `lparam` pointe sur la WINDOWPOS que le
        // système nous laisse modifier.
        let pos = unsafe { &mut *(lparam as *mut WINDOWPOS) };
        if pos.flags & SWP_NOZORDER == 0 {
            pos.hwndInsertAfter = HWND_BOTTOM;
        }
    }
    // SAFETY: on rend la main à la procédure d'origine, avec les mêmes arguments.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

pub fn make_surface(hwnd: HWND, frame: Rect, level: SurfaceLevel) {
    // SAFETY: `hwnd` est une fenêtre vivante de ce processus (créée par winit).
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let ex = (ex | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex as isize);
        if owner_mode() && level == SurfaceLevel::DesktopIcons {
            let desktop = progman();
            if !desktop.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, desktop as isize);
            }
        }
        // Une seule sous-classe par fenêtre : SetWindowSubclass remplace la nôtre si
        // elle existe déjà (même identifiant).
        SetWindowSubclass(hwnd, Some(keep_at_bottom), SUBCLASS_ID, 0);
        let (x, y, w, h) = screens::to_physical(frame);
        SetWindowPos(
            hwnd,
            HWND_BOTTOM,
            x,
            y,
            w,
            h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}
