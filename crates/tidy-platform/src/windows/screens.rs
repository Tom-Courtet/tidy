//! Écrans sous Windows.
//!
//! Chaque écran a son échelle (DPI effectif / 96). Un point vaut un pixel physique
//! divisé par l'échelle de son écran : le cadre logique d'un écran est son cadre
//! physique divisé par son échelle. Pour placer une fenêtre, on repasse en pixels
//! physiques avec l'échelle de l'écran qui la contient ([`to_physical`]).
//!
//! Identifiant stable : le chemin d'interface du moniteur (`\\?\DISPLAY#DEL40F4#…`), qui
//! survit aux redémarrages et au changement d'ordre des écrans.

use tidy_core::geometry::{Point, Rect};
use tidy_core::screen::{Screen, ScreenId};
use windows_sys::Win32::Foundation::{LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    DISPLAY_DEVICEW, EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR,
    MONITORINFO, MONITORINFOEXW,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows_sys::core::BOOL;

use super::from_wide;
use crate::convert::{PhysicalScreen, physical_to_points};

/// `EDD_GET_DEVICE_INTERFACE_NAME` : demander le chemin d'interface du moniteur.
const EDD_GET_DEVICE_INTERFACE_NAME: u32 = 1;
/// `MONITORINFOF_PRIMARY`.
const PRIMARY: u32 = 1;

/// Un écran, avec son cadre physique.
#[derive(Clone)]
pub struct Monitor {
    pub screen: Screen,
    /// Cadre complet en pixels physiques.
    pub physical: RECT,
}

fn rect_of(r: &RECT, scale: f32) -> Rect {
    Rect::new(
        r.left as f32 / scale,
        r.top as f32 / scale,
        (r.right - r.left) as f32 / scale,
        (r.bottom - r.top) as f32 / scale,
    )
}

fn monitor_info(handle: HMONITOR) -> Option<Monitor> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: `info` est une MONITORINFOEXW dont `cbSize` annonce la taille.
    let ok =
        unsafe { GetMonitorInfoW(handle, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO) };
    if ok == 0 {
        return None;
    }
    let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
    // SAFETY: écran valide (fourni par EnumDisplayMonitors), sorties locales.
    let hr = unsafe { GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
    let scale = if hr == 0 && dpi_x > 0 {
        dpi_x as f32 / 96.0
    } else {
        1.0
    };
    let (id, name) = device_identity(&info.szDevice);
    let m = &info.monitorInfo;
    Some(Monitor {
        screen: Screen {
            id: ScreenId(id),
            name,
            frame: rect_of(&m.rcMonitor, scale),
            visible_frame: rect_of(&m.rcWork, scale),
            scale,
            primary: m.dwFlags & PRIMARY != 0,
        },
        physical: m.rcMonitor,
    })
}

/// Identifiant stable et nom lisible du moniteur branché sur l'adaptateur `device`
/// (`\\.\DISPLAY1`). Repli : le nom de l'adaptateur.
fn device_identity(device: &[u16; 32]) -> (String, String) {
    let adapter = from_wide(device);
    let mut dd = DISPLAY_DEVICEW {
        cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
        ..Default::default()
    };
    // SAFETY: `device` est terminé par un zéro (tableau de l'API) ; `dd.cb` annonce la
    // taille de la structure.
    let ok =
        unsafe { EnumDisplayDevicesW(device.as_ptr(), 0, &mut dd, EDD_GET_DEVICE_INTERFACE_NAME) };
    if ok == 0 {
        return (adapter.clone(), adapter);
    }
    let id = from_wide(&dd.DeviceID);
    let name = from_wide(&dd.DeviceString);
    (
        if id.is_empty() { adapter.clone() } else { id },
        if name.is_empty() { adapter } else { name },
    )
}

/// Écrans branchés.
pub fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn collect(
        handle: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        // SAFETY: `data` est le `&mut Vec` passé à EnumDisplayMonitors ci-dessous.
        let list = unsafe { &mut *(data as *mut Vec<HMONITOR>) };
        list.push(handle);
        1
    }
    let mut handles: Vec<HMONITOR> = Vec::new();
    // SAFETY: le rappel ne vit que pendant l'appel, comme `handles`.
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect),
            &mut handles as *mut Vec<HMONITOR> as LPARAM,
        )
    };
    let mut list: Vec<Monitor> = handles.into_iter().filter_map(monitor_info).collect();
    // L'écran principal d'abord, comme sous macOS.
    list.sort_by_key(|m| !m.screen.primary);
    list
}

/// Écran qui contient le centre de `rect` (cadres logiques), sinon le plus proche.
fn monitor_for(monitors: &[Monitor], rect: Rect) -> Option<&Monitor> {
    let center = Point {
        x: rect.x + rect.width / 2.0,
        y: rect.y + rect.height / 2.0,
    };
    monitors
        .iter()
        .find(|m| m.screen.frame.contains(center))
        .or_else(|| {
            monitors.iter().min_by(|a, b| {
                distance(&a.screen.frame, center).total_cmp(&distance(&b.screen.frame, center))
            })
        })
}

fn distance(frame: &Rect, p: Point) -> f32 {
    let dx = (frame.x - p.x).max(0.0).max(p.x - frame.right());
    let dy = (frame.y - p.y).max(0.0).max(p.y - frame.bottom());
    dx * dx + dy * dy
}

/// Cadre physique (x, y, largeur, hauteur) d'un rectangle logique.
pub fn to_physical(rect: Rect) -> (i32, i32, i32, i32) {
    let monitors = monitors();
    let Some(m) = monitor_for(&monitors, rect) else {
        return (
            rect.x as i32,
            rect.y as i32,
            rect.width as i32,
            rect.height as i32,
        );
    };
    let s = m.screen.scale;
    let f = m.screen.frame;
    (
        m.physical.left + ((rect.x - f.x) * s).round() as i32,
        m.physical.top + ((rect.y - f.y) * s).round() as i32,
        (rect.width * s).round() as i32,
        (rect.height * s).round() as i32,
    )
}

/// Point en pixels physiques → points, avec l'échelle de son écran.
pub fn to_points(p: POINT) -> Point {
    let screens: Vec<PhysicalScreen> = monitors()
        .iter()
        .map(|m| PhysicalScreen {
            left: m.physical.left,
            top: m.physical.top,
            right: m.physical.right,
            bottom: m.physical.bottom,
            origin: m.screen.frame.origin(),
            scale: m.screen.scale,
        })
        .collect();
    physical_to_points(p.x, p.y, &screens)
}

/// Position de la souris en points.
pub fn mouse_location() -> Point {
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: sortie locale.
    unsafe { GetCursorPos(&mut p) };
    to_points(p)
}
