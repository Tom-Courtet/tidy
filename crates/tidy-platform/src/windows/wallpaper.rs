//! Fonds d'écran sous Windows (lot J).
//!
//! - **Fonds fixes** : `IDesktopWallpaper` (COM), une image par écran, ajustement
//!   (remplir, ajuster, étirer, centrer, mosaïque : un seul réglage pour tous les écrans,
//!   limite de Windows) et couleur de fond.
//! - **Fonds animés** : la fenêtre du lecteur est placée **derrière les icônes** par
//!   la technique connue des fonds animés : le message `0x052C` demande à `Progman` de
//!   créer une fenêtre `WorkerW` entre le fond et les icônes ; on y accroche la nôtre
//!   en fenêtre enfant. Depuis Windows 11 24H2, les icônes (`SHELLDLL_DefView`) et ce
//!   `WorkerW` sont des enfants de `Progman` : la fenêtre devient alors enfant de
//!   `Progman`, juste sous les icônes. `TIDY_WALLPAPER_HOST=workerw` force l'enfant
//!   `WorkerW` de `Progman` (essai si la première façon ne marche pas sur une machine).
//! - **Pause intelligente** : appli en plein écran (`SHQueryUserNotificationState`,
//!   plus une fenêtre au premier plan qui couvre tout son écran), batterie
//!   (`GetSystemPowerStatus`), session verrouillée (`OpenInputDesktop` refusé).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use tidy_core::geometry::Rect;
use tidy_core::screen::ScreenId;
use tidy_core::settings::wallpaper::{Fit, OriginalScreen, OriginalWallpaper};
use windows::Win32::Foundation::COLORREF;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Shell::{
    DESKTOP_WALLPAPER_POSITION, DWPOS_CENTER, DWPOS_FILL, DWPOS_FIT, DWPOS_STRETCH, DWPOS_TILE,
    DesktopWallpaper, IDesktopWallpaper,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromWindow, ScreenToClient,
};
use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows_sys::Win32::System::StationsAndDesktops::{
    CloseDesktop, DESKTOP_SWITCHDESKTOP, OpenInputDesktop,
};
use windows_sys::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GWL_EXSTYLE, GWL_STYLE, GetClassNameW,
    GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, HWND_BOTTOM, SMTO_NORMAL,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SendMessageTimeoutW, SetParent, SetWindowLongPtrW,
    SetWindowPos, WS_CAPTION, WS_CHILD, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_POPUP, WS_THICKFRAME,
};
use windows_sys::core::BOOL;

use super::icons::com_init;
use super::{from_wide, screens, wide};
use crate::SystemActivity;

// --- Fonds fixes --------------------------------------------------------------

fn api() -> Result<IDesktopWallpaper> {
    com_init();
    // SAFETY: COM initialisé sur ce thread ; classe documentée du Shell.
    unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL) }
        .context("IDesktopWallpaper indisponible")
}

/// Texte rendu par le Shell, libéré ensuite.
fn take(text: PWSTR) -> String {
    if text.is_null() {
        return String::new();
    }
    // SAFETY: chaîne terminée par un zéro allouée par le Shell, libérée une seule
    // fois, après la copie.
    unsafe {
        let s = text.to_string().unwrap_or_default();
        CoTaskMemFree(Some(text.0 as *const _));
        s
    }
}

struct MonitorPath {
    id: String,
    rect: (i32, i32, i32, i32),
}

/// Écrans connus d'`IDesktopWallpaper` (chemin du moniteur et cadre physique).
fn monitor_paths(api: &IDesktopWallpaper) -> Vec<MonitorPath> {
    // SAFETY: appels COM simples ; chaque chemin rendu est libéré par `take`.
    let count = unsafe { api.GetMonitorDevicePathCount() }.unwrap_or(0);
    (0..count)
        .filter_map(|i| {
            // SAFETY: indice dans les bornes données par le Shell.
            let id = take(unsafe { api.GetMonitorDevicePathAt(i) }.ok()?);
            let wid = HSTRING::from(id.as_str());
            // SAFETY: `wid` vit pendant l'appel.
            let r = unsafe { api.GetMonitorRECT(&wid) }.ok()?;
            Some(MonitorPath {
                id,
                rect: (r.left, r.top, r.right, r.bottom),
            })
        })
        .collect()
}

/// Chemin `IDesktopWallpaper` de l'écran : l'identifiant de Tidy est le même chemin
/// d'interface ; à défaut, l'écran de même cadre physique.
fn monitor_for(api: &IDesktopWallpaper, screen: &ScreenId) -> Option<String> {
    let paths = monitor_paths(api);
    if let Some(p) = paths.iter().find(|p| p.id.eq_ignore_ascii_case(&screen.0)) {
        return Some(p.id.clone());
    }
    let m = screens::monitors()
        .into_iter()
        .find(|m| m.screen.id == *screen)?;
    let r = m.physical;
    paths
        .into_iter()
        .find(|p| p.rect == (r.left, r.top, r.right, r.bottom))
        .map(|p| p.id)
}

fn position(fit: Fit) -> DESKTOP_WALLPAPER_POSITION {
    match fit {
        Fit::Fill => DWPOS_FILL,
        Fit::Fit => DWPOS_FIT,
        Fit::Stretch => DWPOS_STRETCH,
        Fit::Center => DWPOS_CENTER,
        Fit::Tile => DWPOS_TILE,
    }
}

fn fit_of(pos: DESKTOP_WALLPAPER_POSITION) -> Option<Fit> {
    Some(match pos {
        DWPOS_FILL => Fit::Fill,
        DWPOS_FIT => Fit::Fit,
        DWPOS_STRETCH => Fit::Stretch,
        DWPOS_CENTER => Fit::Center,
        DWPOS_TILE => Fit::Tile,
        _ => return None,
    })
}

fn colorref([r, g, b]: [u8; 3]) -> COLORREF {
    COLORREF(u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16))
}

fn rgb_of(c: COLORREF) -> [u8; 3] {
    [c.0 as u8, (c.0 >> 8) as u8, (c.0 >> 16) as u8]
}

pub fn snapshot() -> Option<OriginalWallpaper> {
    let api = api().inspect_err(|e| log::warn!("{e:#}")).ok()?;
    // SAFETY: lectures simples.
    let fit = unsafe { api.GetPosition() }.ok().and_then(fit_of);
    // SAFETY: lecture simple.
    let background = unsafe { api.GetBackgroundColor() }.ok().map(rgb_of);
    let screens = monitor_paths(&api)
        .into_iter()
        .map(|m| {
            let wid = HSTRING::from(m.id.as_str());
            // SAFETY: `wid` vit pendant l'appel ; la chaîne rendue est libérée.
            let image = unsafe { api.GetWallpaper(&wid) }
                .ok()
                .map(take)
                .filter(|p| !p.is_empty())
                .map(PathBuf::from);
            OriginalScreen {
                screen: m.id,
                image,
                fit,
                background,
            }
        })
        .collect();
    Some(OriginalWallpaper {
        screens,
        fit,
        background,
        ..Default::default()
    })
}

pub fn set(screen: Option<&ScreenId>, image: &Path, fit: Fit, background: [u8; 3]) -> Result<()> {
    if !image.is_file() {
        bail!("image introuvable : {}", image.display());
    }
    let api = api()?;
    let path = HSTRING::from(image.as_os_str());
    let monitor = match screen {
        Some(id) => Some(HSTRING::from(
            monitor_for(&api, id).with_context(|| format!("écran {} introuvable", id.0))?,
        )),
        None => None,
    };
    // SAFETY: appels COM documentés ; les chaînes vivent pendant les appels ; un
    // moniteur nul vise tous les écrans.
    unsafe {
        let _ = api.Enable(true);
        let _ = api.SetBackgroundColor(colorref(background));
        api.SetPosition(position(fit))
            .context("ajustement du fond refusé")?;
        match &monitor {
            Some(m) => api.SetWallpaper(m, &path),
            None => api.SetWallpaper(PCWSTR::null(), &path),
        }
        .context("fond d'écran refusé par Windows")?;
    }
    Ok(())
}

pub fn restore(original: &OriginalWallpaper) -> Result<()> {
    let api = api()?;
    // SAFETY: appels COM documentés ; les chaînes vivent pendant les appels.
    unsafe {
        if let Some(c) = original.background {
            let _ = api.SetBackgroundColor(colorref(c));
        }
        if let Some(fit) = original.fit {
            let _ = api.SetPosition(position(fit));
        }
    }
    let known: Vec<String> = monitor_paths(&api).into_iter().map(|m| m.id).collect();
    let mut restored = 0;
    let mut fallback: Option<&Path> = None;
    for s in &original.screens {
        let Some(image) = &s.image else {
            continue;
        };
        fallback.get_or_insert(image);
        if !known.iter().any(|k| k.eq_ignore_ascii_case(&s.screen)) || !image.is_file() {
            continue;
        }
        let (id, path) = (
            HSTRING::from(s.screen.as_str()),
            HSTRING::from(image.as_os_str()),
        );
        // SAFETY: chaînes vivantes pendant l'appel.
        if unsafe { api.SetWallpaper(&id, &path) }.is_ok() {
            restored += 1;
        }
    }
    if restored == 0 {
        match fallback.filter(|p| p.is_file()) {
            // Écrans changés depuis : la première image d'origine partout.
            Some(image) => {
                let path = HSTRING::from(image.as_os_str());
                // SAFETY: chaîne vivante pendant l'appel ; tous les écrans.
                unsafe { api.SetWallpaper(PCWSTR::null(), &path) }
                    .context("fond d'origine refusé par Windows")?;
            }
            // Couleur unie à l'origine.
            None => {
                // SAFETY: appel simple.
                unsafe { api.Enable(false) }.context("couleur unie refusée")?;
            }
        }
    }
    Ok(())
}

/// Repose le fond de chaque écran tel quel : Windows redessine le bureau.
pub fn refresh() {
    let Ok(api) = api() else {
        return;
    };
    for m in monitor_paths(&api) {
        let id = HSTRING::from(m.id.as_str());
        // SAFETY: chaînes vivantes pendant les appels ; la chaîne rendue est libérée.
        unsafe {
            let Ok(current) = api.GetWallpaper(&id) else {
                continue;
            };
            let current = take(current);
            if !current.is_empty() {
                let _ = api.SetWallpaper(&id, &HSTRING::from(current.as_str()));
            }
        }
    }
}

// --- Fonds animés : fenêtre derrière les icônes -------------------------------------

fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    // SAFETY: tampon local, taille passée à l'API.
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 0 {
        return String::new();
    }
    from_wide(&buf[..n as usize])
}

fn child(parent: HWND, after: HWND, class: &str) -> HWND {
    let class = wide(class);
    // SAFETY: nom de classe terminé par un zéro ; parent nul = premier niveau.
    unsafe { FindWindowExW(parent, after, class.as_ptr(), std::ptr::null()) }
}

/// Windows 10 et 11 avant 24H2 : le `WorkerW` de premier niveau qui suit celui qui
/// porte les icônes.
fn classic_worker() -> HWND {
    unsafe extern "system" fn visit(hwnd: HWND, data: isize) -> BOOL {
        // SAFETY: `data` est le `&mut HWND` passé à EnumWindows ci-dessous.
        let found = unsafe { &mut *(data as *mut HWND) };
        if !child(hwnd, std::ptr::null_mut(), "SHELLDLL_DefView").is_null() {
            let worker = child(std::ptr::null_mut(), hwnd, "WorkerW");
            if !worker.is_null() {
                *found = worker;
                return 0;
            }
        }
        1
    }
    let mut found: HWND = std::ptr::null_mut();
    // SAFETY: le rappel ne vit que pendant l'appel, comme `found`.
    unsafe { EnumWindows(Some(visit), &mut found as *mut HWND as isize) };
    found
}

/// Accroche `hwnd` derrière les icônes, sur `frame` (points).
pub fn attach(hwnd: HWND, frame: Rect) -> bool {
    let progman = {
        let class = wide("Progman");
        // SAFETY: nom de classe terminé par un zéro.
        unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) }
    };
    if progman.is_null() {
        log::warn!("fond animé : Progman introuvable");
        return false;
    }
    let mut result = 0usize;
    // SAFETY: message non documenté mais stable depuis Windows 8 (création du
    // WorkerW) ; délai borné pour ne pas bloquer si l'Explorateur ne répond pas.
    unsafe {
        SendMessageTimeoutW(progman, 0x052C, 0xD, 0x1, SMTO_NORMAL, 1000, &mut result);
        SendMessageTimeoutW(progman, 0x052C, 0, 0, SMTO_NORMAL, 1000, &mut result);
    }
    let classic = classic_worker();
    let icons = child(progman, std::ptr::null_mut(), "SHELLDLL_DefView");
    let inner_worker = child(progman, std::ptr::null_mut(), "WorkerW");
    let force_worker = std::env::var("TIDY_WALLPAPER_HOST").as_deref() == Ok("workerw");
    // (parent, fenêtre au-dessus de la nôtre)
    let (parent, above) = if !classic.is_null() && classic != progman {
        (classic, None)
    } else if force_worker && !inner_worker.is_null() {
        (inner_worker, None)
    } else if !icons.is_null() {
        (progman, Some(icons))
    } else {
        (progman, None)
    };
    log::info!(
        "fond animé : hôte {} ({})",
        class_of(parent),
        if above.is_some() {
            "sous les icônes, dans Progman"
        } else {
            "WorkerW"
        }
    );
    let (x, y, w, h) = screens::to_physical(frame);
    let mut origin = POINT { x, y };
    // SAFETY: `hwnd` est une fenêtre vivante de ce processus, `parent` une fenêtre de
    // l'Explorateur ; `origin` est local.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let style = (style & !(WS_POPUP | WS_CAPTION | WS_THICKFRAME)) | WS_CHILD;
        SetWindowLongPtrW(hwnd, GWL_STYLE, style as isize);
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            ((ex | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW) as isize,
        );
        if SetParent(hwnd, parent).is_null() {
            log::warn!("fond animé : SetParent refusé");
            return false;
        }
        ScreenToClient(parent, &mut origin);
        SetWindowPos(
            hwnd,
            above.unwrap_or(HWND_BOTTOM),
            origin.x,
            origin.y,
            w,
            h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
    true
}

// --- Pause intelligente -----------------------------------------------------------

/// La fenêtre au premier plan couvre-t-elle tout son écran (hors bureau et barre des
/// tâches) ?
fn foreground_covers_screen() -> bool {
    // SAFETY: lectures d'état ; structures locales dont la taille est annoncée.
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return false;
        }
        if matches!(
            class_of(fg).as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        ) {
            return false;
        }
        let mut r = RECT::default();
        if GetWindowRect(fg, &mut r) == 0 {
            return false;
        }
        let monitor = MonitorFromWindow(fg, MONITOR_DEFAULTTONULL);
        if monitor.is_null() {
            return false;
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return false;
        }
        let m = info.rcMonitor;
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    }
}

pub fn activity() -> SystemActivity {
    let mut state = 0;
    // SAFETY: sortie locale.
    let quns = unsafe { SHQueryUserNotificationState(&mut state) } == 0;
    let fullscreen = (quns
        && matches!(
            state,
            QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE
        ))
        || foreground_covers_screen();

    let mut power = SYSTEM_POWER_STATUS::default();
    // SAFETY: sortie locale.
    let power_ok = unsafe { GetSystemPowerStatus(&mut power) } != 0;
    // ACLineStatus : 0 = sur batterie ; BatteryFlag 128 = pas de batterie.
    let on_battery = power_ok && power.ACLineStatus == 0 && power.BatteryFlag != 128;
    let battery_percent =
        (power_ok && power.BatteryLifePercent <= 100).then_some(power.BatteryLifePercent);

    // Session verrouillée (ou écran sécurisé) : le bureau d'entrée n'est pas le nôtre.
    // SAFETY: le bureau ouvert est refermé aussitôt.
    let locked = unsafe {
        let desk = OpenInputDesktop(0, 0, DESKTOP_SWITCHDESKTOP);
        if desk.is_null() {
            true
        } else {
            CloseDesktop(desk);
            false
        }
    };
    SystemActivity {
        fullscreen,
        on_battery,
        battery_percent,
        locked,
    }
}
