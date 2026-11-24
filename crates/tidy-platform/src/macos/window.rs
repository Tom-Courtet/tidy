//! Fenêtres posées sur le bureau.
//!
//! Juste au-dessus des icônes du bureau (`kCGDesktopIconWindowLevel + 1`) : sous toutes
//! les fenêtres des applis, au-dessus du fond d'écran et des icônes du Finder. Sur
//! tous les Spaces, immobiles pendant Mission Control et « Afficher le bureau », hors
//! du cycle ⌘` et des fenêtres de l'app.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSFloatingWindowLevel, NSRunningApplication,
    NSView, NSWindow, NSWindowCollectionBehavior,
};
use objc2_core_graphics::{CGWindowLevelForKey, CGWindowLevelKey};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tidy_core::geometry::Rect;

use super::screens::to_appkit;
use crate::SurfaceLevel;

fn ns_window(window: &dyn HasWindowHandle) -> Option<Retained<NSWindow>> {
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(h) = handle.as_raw() else {
        return None;
    };
    // SAFETY: winit fournit un pointeur valide vers la NSView de la fenêtre, vivante
    // tant que la fenêtre existe (l'appelant tient la fenêtre).
    let view: &NSView = unsafe { h.ns_view.cast().as_ref() };
    view.window()
}

fn level(level: SurfaceLevel) -> isize {
    let key = match level {
        SurfaceLevel::DesktopIcons => CGWindowLevelKey::DesktopIconWindowLevelKey,
        SurfaceLevel::Wallpaper => CGWindowLevelKey::DesktopWindowLevelKey,
    };
    let base = CGWindowLevelForKey(key) as isize;
    match level {
        SurfaceLevel::DesktopIcons => base + 1,
        SurfaceLevel::Wallpaper => base,
    }
}

fn configure(win: &NSWindow, frame: Rect, surface: SurfaceLevel) {
    win.setLevel(level(surface));
    win.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    win.setHasShadow(false);
    win.setHidesOnDeactivate(false);
    win.setFrame_display(to_appkit(frame), true);
    win.orderFrontRegardless();
}

pub fn make_surface(window: &dyn HasWindowHandle, frame: Rect, surface: SurfaceLevel) {
    if MainThreadMarker::new().is_none() {
        log::warn!("surface du bureau : pas sur le thread principal");
        return;
    }
    match ns_window(window) {
        Some(win) => configure(&win, frame, surface),
        None => log::warn!("surface du bureau : fenêtre native introuvable"),
    }
}

/// La plus récente des fenêtres visibles de ce titre (une fenêtre fermée peut rester un
/// instant dans la liste).
pub fn find_titled(mtm: MainThreadMarker, title: &str) -> Option<Retained<NSWindow>> {
    NSApplication::sharedApplication(mtm)
        .windows()
        .iter()
        .filter(|w| w.isVisible() && w.title().to_string() == title)
        .last()
}

pub fn make_surface_titled(title: &str, frame: Rect, surface: SurfaceLevel) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    match find_titled(mtm, title) {
        Some(win) => {
            configure(&win, frame, surface);
            true
        }
        None => false,
    }
}

/// Accès rapide : fenêtre flottante au-dessus des applis (`raised`), ou rendue au
/// niveau du bureau.
pub fn raise(title: &str, raised: bool) {
    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("accès rapide : pas sur le thread principal");
        return;
    };
    let Some(win) = find_titled(mtm, title) else {
        log::debug!("accès rapide : fenêtre « {title} » introuvable");
        return;
    };
    if raised {
        win.setLevel(NSFloatingWindowLevel);
        win.orderFrontRegardless();
        win.makeKeyWindow();
        NSRunningApplication::currentApplication()
            .activateWithOptions(NSApplicationActivationOptions::empty());
    } else {
        win.setLevel(level(SurfaceLevel::DesktopIcons));
        win.orderFrontRegardless();
    }
}
