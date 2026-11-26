//! Session et veille (lot F, boîte privée) :
//! - verrouillage et déverrouillage de la session (`WTSRegisterSessionNotification`,
//!   message `WM_WTSSESSION_CHANGE`) ;
//! - mise en veille et réveil (`WM_POWERBROADCAST`, diffusé aux fenêtres de premier
//!   niveau : une fenêtre « de messages seulement » ne le recevrait pas) ;
//! - inactivité (`GetLastInputInfo`).
//!
//! Comme pour les écrans ([`super::watch`]), une fenêtre cachée de premier niveau,
//! créée sur le thread qui pompe les messages, reçoit tout cela.

use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
};
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND, PBT_APMSUSPEND,
    RegisterClassExW, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSEXW, WS_EX_TOOLWINDOW,
    WS_POPUP, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};

use super::wide;
use crate::{SessionCallback, SessionEvent};

static ON_EVENT: OnceLock<SessionCallback> = OnceLock::new();

/// Événement correspondant à un message, s'il y en a un.
fn event_of(msg: u32, wparam: WPARAM) -> Option<SessionEvent> {
    let code = u32::try_from(wparam).ok()?;
    match (msg, code) {
        (WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK) => Some(SessionEvent::Locked),
        (WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK) => Some(SessionEvent::Unlocked),
        (WM_POWERBROADCAST, PBT_APMSUSPEND) => Some(SessionEvent::Sleep),
        (WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND) => {
            Some(SessionEvent::Wake)
        }
        _ => None,
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if let Some(event) = event_of(msg, wparam)
        && let Some(callback) = ON_EVENT.get()
    {
        callback(event);
    }
    if msg == WM_POWERBROADCAST {
        // TRUE : on ne s'oppose à rien.
        return 1;
    }
    // SAFETY: traitement par défaut, mêmes arguments.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Crée la fenêtre cachée sur le thread appelant (qui doit pomper les messages).
pub fn watch_session(on_event: SessionCallback) -> bool {
    if ON_EVENT.set(on_event).is_err() {
        return true;
    }
    let class = wide("TidySessionWatch");
    let title = wide("Tidy · session");
    // SAFETY: classe et fenêtre de ce module ; les chaînes vivent pendant les appels.
    // La fenêtre n'est jamais montrée et vit jusqu'à la fin du processus (la
    // notification de session se désinscrit avec elle).
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
            log::warn!("surveillance de la session : classe de fenêtre refusée");
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
            log::warn!("surveillance de la session : fenêtre cachée refusée");
            return false;
        }
        if WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) == 0 {
            // La veille reste surveillée.
            log::warn!("surveillance de la session : verrouillage non suivi");
        }
    }
    true
}

/// Secondes depuis la dernière entrée clavier ou souris de la session.
pub fn idle_seconds() -> Option<f64> {
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: structure initialisée avec sa taille, comme l'exige l'API.
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return None;
    }
    // SAFETY: sans argument ni effet de bord.
    let now = unsafe { GetTickCount() };
    // Les deux compteurs bouclent tous les 49,7 jours : la différence modulo 2³² reste
    // juste.
    Some(f64::from(now.wrapping_sub(info.dwTime)) / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_map_to_events() {
        assert_eq!(
            event_of(WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK as WPARAM),
            Some(SessionEvent::Locked)
        );
        assert_eq!(
            event_of(WM_POWERBROADCAST, PBT_APMSUSPEND as WPARAM),
            Some(SessionEvent::Sleep)
        );
        assert_eq!(
            event_of(WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC as WPARAM),
            Some(SessionEvent::Wake)
        );
        assert_eq!(event_of(WM_POWERBROADCAST, 10), None);
    }
}
