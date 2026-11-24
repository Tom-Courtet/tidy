//! Changements d'écrans : branchement, résolution, échelle, zone de travail (barre des
//! tâches déplacée ou redimensionnée).
//!
//! Une fenêtre **cachée de premier niveau** reçoit les messages diffusés par le système
//! (`WM_DISPLAYCHANGE`, `WM_SETTINGCHANGE`, `WM_DEVICECHANGE`, et `WM_DPICHANGED`
//! quand il lui est adressé). Une fenêtre « de messages seulement » ne les recevrait
//! pas. Ces messages arrivent en rafale : on attend un court instant de calme, puis on
//! compare la liste des écrans à la précédente ; le rappel n'est appelé que si elle a
//! changé (cadres, cadres utiles, échelles, identifiants).

use std::cell::RefCell;
use std::sync::OnceLock;

use tidy_core::screen::Screen;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, KillTimer, RegisterClassExW, SetTimer, WM_DEVICECHANGE,
    WM_DISPLAYCHANGE, WM_DPICHANGED, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSEXW, WS_EX_TOOLWINDOW,
    WS_POPUP,
};

use super::{screens, wide};
use crate::Callback;

/// Minuterie d'attente du calme.
const TIMER_ID: usize = 1;
/// Attente après le dernier message, en millisecondes.
const SETTLE_MS: u32 = 400;

static ON_CHANGE: OnceLock<Callback> = OnceLock::new();

thread_local! {
    /// Écrans connus au dernier appel (thread de la fenêtre cachée).
    static LAST: RefCell<Vec<Screen>> = const { RefCell::new(Vec::new()) };
}

fn current() -> Vec<Screen> {
    screens::monitors().into_iter().map(|m| m.screen).collect()
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_DISPLAYCHANGE | WM_DPICHANGED | WM_SETTINGCHANGE | WM_DEVICECHANGE => {
            // SAFETY: notre fenêtre ; une minuterie de même numéro est remplacée (le délai
            // repart de zéro).
            unsafe { SetTimer(hwnd, TIMER_ID, SETTLE_MS, None) };
        }
        WM_TIMER if wparam == TIMER_ID => {
            // SAFETY: minuterie posée ci-dessus sur notre fenêtre.
            unsafe { KillTimer(hwnd, TIMER_ID) };
            let now = current();
            let changed = LAST.with(|last| {
                let mut last = last.borrow_mut();
                let changed = *last != now;
                *last = now;
                changed
            });
            if changed && let Some(callback) = ON_CHANGE.get() {
                callback();
            }
            return 0;
        }
        _ => {}
    }
    // SAFETY: traitement par défaut, mêmes arguments.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Crée la fenêtre cachée sur le thread appelant (qui doit pomper les messages).
pub fn watch_screens(on_change: Callback) -> bool {
    if ON_CHANGE.set(on_change).is_err() {
        return true;
    }
    LAST.with(|last| *last.borrow_mut() = current());
    let class = wide("TidyScreenWatch");
    let title = wide("Tidy · écrans");
    // SAFETY: classe et fenêtre de ce module ; les chaînes vivent pendant les appels.
    // La fenêtre n'est jamais montrée et vit jusqu'à la fin du processus.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            log::warn!("surveillance des écrans : classe de fenêtre refusée");
            return false;
        }
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            log::warn!("surveillance des écrans : fenêtre cachée refusée");
            return false;
        }
    }
    true
}
