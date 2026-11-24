//! Écrans : identifiant stable, cadres en coordonnées globales CoreGraphics.

use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;
use objc2_core_graphics::{
    CGDirectDisplayID, CGDisplayBounds, CGDisplayIsBuiltin, CGDisplayModelNumber,
    CGDisplaySerialNumber, CGDisplayVendorNumber, CGGetActiveDisplayList, CGMainDisplayID,
};
use objc2_foundation::{NSNumber, NSRect, ns_string};
use tidy_core::geometry::Rect;
use tidy_core::screen::{Screen, ScreenId};

const MAX_DISPLAYS: usize = 16;

/// Hauteur de l'écran principal, en points (pour passer des coordonnées AppKit, y vers
/// le haut, aux coordonnées CoreGraphics, y vers le bas).
pub fn main_height() -> f64 {
    CGDisplayBounds(CGMainDisplayID()).size.height
}

/// Cadre AppKit → [`Rect`] CoreGraphics.
pub fn from_appkit(r: NSRect) -> Rect {
    Rect::new(
        r.origin.x as f32,
        (main_height() - (r.origin.y + r.size.height)) as f32,
        r.size.width as f32,
        r.size.height as f32,
    )
}

/// [`Rect`] CoreGraphics → cadre AppKit.
pub fn to_appkit(r: Rect) -> NSRect {
    let y = main_height() - f64::from(r.y) - f64::from(r.height);
    NSRect::new(
        objc2_foundation::NSPoint::new(f64::from(r.x), y),
        objc2_foundation::NSSize::new(f64::from(r.width), f64::from(r.height)),
    )
}

fn active_displays() -> Vec<CGDirectDisplayID> {
    let mut ids = [0 as CGDirectDisplayID; MAX_DISPLAYS];
    let mut count = 0u32;
    // SAFETY : `ids` a la place de MAX_DISPLAYS identifiants, `count` est valide.
    let err =
        unsafe { CGGetActiveDisplayList(MAX_DISPLAYS as u32, ids.as_mut_ptr(), &mut count) };
    if err.0 != 0 {
        log::warn!("liste des écrans indisponible (erreur {})", err.0);
        return vec![CGMainDisplayID()];
    }
    ids[..count as usize].to_vec()
}

/// Identifiant stable : fabricant, modèle et numéro de série de l'écran.
fn stable_id(display: CGDirectDisplayID) -> String {
    format!(
        "{:04x}-{:04x}-{:08x}",
        CGDisplayVendorNumber(display),
        CGDisplayModelNumber(display),
        CGDisplaySerialNumber(display)
    )
}

/// Données que seul AppKit donne (sur le thread principal) : cadre utile, échelle, nom.
struct AppKitInfo {
    visible_frame: Rect,
    scale: f32,
    name: String,
}

fn appkit_info(display: CGDirectDisplayID) -> Option<AppKitInfo> {
    let mtm = MainThreadMarker::new()?;
    NSScreen::screens(mtm).iter().find_map(|screen| {
        let number = screen
            .deviceDescription()
            .objectForKey(ns_string!("NSScreenNumber"))?
            .downcast::<NSNumber>()
            .ok()?;
        (number.unsignedIntValue() == display).then(|| AppKitInfo {
            visible_frame: from_appkit(screen.visibleFrame()),
            scale: screen.backingScaleFactor() as f32,
            name: screen.localizedName().to_string(),
        })
    })
}

pub fn list() -> Vec<Screen> {
    let main = CGMainDisplayID();
    let mut screens: Vec<Screen> = active_displays()
        .into_iter()
        .map(|display| {
            let b = CGDisplayBounds(display);
            let frame = Rect::new(
                b.origin.x as f32,
                b.origin.y as f32,
                b.size.width as f32,
                b.size.height as f32,
            );
            let info = appkit_info(display);
            let builtin = CGDisplayIsBuiltin(display);
            Screen {
                id: ScreenId(stable_id(display)),
                name: info.as_ref().map(|i| i.name.clone()).unwrap_or_else(|| {
                    if builtin {
                        "Écran intégré".into()
                    } else {
                        format!("Écran {display}")
                    }
                }),
                frame,
                visible_frame: info.as_ref().map_or(frame, |i| i.visible_frame),
                scale: info.as_ref().map_or(1.0, |i| i.scale),
                primary: display == main,
            }
        })
        .collect();
    // Deux écrans identiques sans numéro de série : on les distingue par leur rang.
    let mut seen = std::collections::HashMap::<String, usize>::new();
    for screen in &mut screens {
        let n = seen.entry(screen.id.0.clone()).or_insert(0);
        if *n > 0 {
            screen.id.0 = format!("{}#{n}", screen.id.0);
        }
        *n += 1;
    }
    screens
}
