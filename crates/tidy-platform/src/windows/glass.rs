//! Fond « verre dépoli » des boîtes, sous Windows (lot M).
//!
//! - **Windows 11 22H2 et plus** : matériau acrylique du système
//!   (`DWMWA_SYSTEMBACKDROP_TYPE` = `DWMSBT_TRANSIENTWINDOW`), étendu à toute la fenêtre
//!   (`DwmExtendFrameIntoClientArea`, marges -1) : là où egui laisse le fond
//!   transparent ou à demi transparent, on voit le flou. Thème sombre et coins arrondis
//!   demandés au système.
//! - **Windows 10** (et Windows 11 plus ancien, qui refuse l'attribut) :
//!   `SetWindowCompositionAttribute` de user32 (non documentée, utilisée par la barre
//!   des tâches) avec `ACCENT_ENABLE_ACRYLICBLURBEHIND`, ou le flou simple
//!   (`ACCENT_ENABLE_BLURBEHIND`) si l'acrylique est refusé.
//!
//! `TIDY_GLASS=blur` force la seconde voie (essai sur une machine où la première ne
//! conviendrait pas, par exemple si l'effet disparaissait quand la boîte n'est pas la
//! fenêtre active). Les fonctions sont chargées à l'exécution : rien ne casse si elles
//! manquent, la boîte garde son fond ordinaire.

use std::ffi::c_void;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};

use super::{wide, window};

// --- dwmapi.dll ---
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
const DWMWCP_DEFAULT: i32 = 0;
const DWMWCP_DONOTROUND: i32 = 1;
const DWMWCP_ROUND: i32 = 2;
const DWMSBT_NONE: i32 = 1;
const DWMSBT_TRANSIENTWINDOW: i32 = 3;

/// `MARGINS` (uxtheme.h).
#[repr(C)]
struct Margins {
    left: i32,
    right: i32,
    top: i32,
    bottom: i32,
}

type DwmSetWindowAttribute = unsafe extern "system" fn(HWND, u32, *const c_void, u32) -> i32;
type DwmExtendFrameIntoClientArea = unsafe extern "system" fn(HWND, *const Margins) -> i32;

// --- user32.dll (non documenté) ---
const WCA_ACCENT_POLICY: u32 = 19;
const ACCENT_DISABLED: u32 = 0;
const ACCENT_ENABLE_BLURBEHIND: u32 = 3;
const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;
/// Teinte de l'acrylique, AABBGGRR : noir à 25 % (egui dessine sa propre teinte
/// par-dessus).
const ACRYLIC_TINT: u32 = 0x4010_1010;

#[repr(C)]
struct AccentPolicy {
    state: u32,
    flags: u32,
    gradient_color: u32,
    animation_id: u32,
}

#[repr(C)]
struct CompositionAttribData {
    attribute: u32,
    data: *mut c_void,
    size: usize,
}

type SetWindowCompositionAttribute =
    unsafe extern "system" fn(HWND, *mut CompositionAttribData) -> i32;

/// Fonctions chargées une fois.
struct Api {
    set_attribute: Option<DwmSetWindowAttribute>,
    extend_frame: Option<DwmExtendFrameIntoClientArea>,
    set_composition: Option<SetWindowCompositionAttribute>,
}

/// Adresse de `name` (terminé par un zéro) dans `dll`, chargée au besoin.
fn proc_address(dll: &str, name: &[u8]) -> Option<unsafe extern "system" fn() -> isize> {
    let dll = wide(dll);
    // SAFETY: nom de module terminé par un zéro ; GetModuleHandleW ne charge rien.
    let mut module = unsafe { GetModuleHandleW(dll.as_ptr()) };
    if module.is_null() {
        // SAFETY: même nom ; une DLL du système, chargée pour la vie du processus.
        module = unsafe { LoadLibraryW(dll.as_ptr()) };
    }
    if module.is_null() {
        return None;
    }
    debug_assert_eq!(name.last(), Some(&0));
    // SAFETY: module valide, nom ASCII terminé par un zéro.
    unsafe { GetProcAddress(module, name.as_ptr()) }
}

fn api() -> &'static Api {
    static API: OnceLock<Api> = OnceLock::new();
    API.get_or_init(|| {
        // SAFETY: chaque adresse est convertie vers la signature documentée de la
        // fonction (ou celle, stable depuis Windows 7, de SetWindowCompositionAttribute).
        unsafe {
            Api {
                set_attribute: proc_address("dwmapi.dll", b"DwmSetWindowAttribute\0")
                    .map(|f| std::mem::transmute::<_, DwmSetWindowAttribute>(f)),
                extend_frame: proc_address("dwmapi.dll", b"DwmExtendFrameIntoClientArea\0")
                    .map(|f| std::mem::transmute::<_, DwmExtendFrameIntoClientArea>(f)),
                set_composition: proc_address("user32.dll", b"SetWindowCompositionAttribute\0")
                    .map(|f| std::mem::transmute::<_, SetWindowCompositionAttribute>(f)),
            }
        }
    })
}

/// Attribut DWM entier. `true` si DWM l'accepte.
fn set_attribute(hwnd: HWND, attribute: u32, value: i32) -> bool {
    let Some(f) = api().set_attribute else {
        return false;
    };
    // SAFETY: `hwnd` est une fenêtre vivante de ce thread ; `value` vit pendant l'appel
    // et sa taille est passée.
    let hr = unsafe {
        f(
            hwnd,
            attribute,
            (&value as *const i32).cast(),
            std::mem::size_of::<i32>() as u32,
        )
    };
    hr >= 0
}

fn extend_frame(hwnd: HWND, all: bool) -> bool {
    let Some(f) = api().extend_frame else {
        return false;
    };
    let m = if all { -1 } else { 0 };
    let margins = Margins {
        left: m,
        right: m,
        top: m,
        bottom: m,
    };
    // SAFETY: fenêtre vivante ; `margins` vit pendant l'appel.
    unsafe { f(hwnd, &margins) >= 0 }
}

fn set_accent(hwnd: HWND, state: u32) -> bool {
    let Some(f) = api().set_composition else {
        return false;
    };
    let mut policy = AccentPolicy {
        state,
        // 2 : la teinte `gradient_color` est utilisée.
        flags: 2,
        gradient_color: ACRYLIC_TINT,
        animation_id: 0,
    };
    let mut data = CompositionAttribData {
        attribute: WCA_ACCENT_POLICY,
        data: (&mut policy as *mut AccentPolicy).cast(),
        size: std::mem::size_of::<AccentPolicy>(),
    };
    // SAFETY: fenêtre vivante ; `policy` et `data` vivent pendant l'appel, tailles
    // exactes.
    unsafe { f(hwnd, &mut data) != 0 }
}

/// Pose ou retire le verre sur la fenêtre `title`. Rend `true` si l'effet est actif.
pub fn set_glass(title: &str, enabled: bool, corner_radius: f32) -> bool {
    let Some(hwnd) = window::find_titled(title) else {
        log::debug!("verre : fenêtre « {title} » introuvable");
        return false;
    };
    if !enabled {
        set_attribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_NONE);
        extend_frame(hwnd, false);
        set_accent(hwnd, ACCENT_DISABLED);
        set_attribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DEFAULT);
        return false;
    }
    // Coins : arrondis par le système (Windows 11 ; sans effet sous Windows 10).
    let corners = if corner_radius >= 1.0 {
        DWMWCP_ROUND
    } else {
        DWMWCP_DONOTROUND
    };
    set_attribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, corners);
    set_attribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, 1);
    let forced_blur = std::env::var("TIDY_GLASS").as_deref() == Ok("blur");
    if !forced_blur
        && set_attribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_TRANSIENTWINDOW)
        && extend_frame(hwnd, true)
    {
        log::debug!("verre : acrylique du système (Windows 11) pour « {title} »");
        return true;
    }
    // Windows 10, ou Windows 11 sans matériau du système.
    set_attribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_NONE);
    extend_frame(hwnd, false);
    if set_accent(hwnd, ACCENT_ENABLE_ACRYLICBLURBEHIND) {
        log::debug!("verre : acrylique (SetWindowCompositionAttribute) pour « {title} »");
        return true;
    }
    if set_accent(hwnd, ACCENT_ENABLE_BLURBEHIND) {
        log::debug!("verre : flou simple pour « {title} »");
        return true;
    }
    log::info!("verre refusé par Windows pour « {title} »");
    false
}
