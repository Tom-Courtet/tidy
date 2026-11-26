//! Rectangle tracé sur le bureau vide (« Créer une boîte en dessinant »), sous macOS
//! (lot M). Même rôle que le crochet souris de Windows (`windows/desktop.rs`).
//!
//! Un moniteur d'événements **global** (`NSEvent`) voit les clics envoyés aux autres
//! applis, dont le Finder qui dessine le bureau, **sans les intercepter** : la
//! sélection d'icônes du Finder marche comme avant. Il ne voit pas les clics sur les
//! fenêtres de ce processus (les boîtes). Mouvements de souris seulement : aucune
//! autorisation d'accessibilité n'est demandée.
//!
//! 1. Appui : retenu seulement s'il tombe sur le bureau, c'est-à-dire sur aucune
//!    fenêtre au-dessus des icônes du bureau (liste des fenêtres de CoreGraphics, sans
//!    les éléments du bureau) ;
//! 2. glisser d'une icône : le Finder remplit le presse-papiers de glisser
//!    (`NSPasteboardNameDrag`), dont le compteur change ; un rectangle de sélection n'y
//!    touche pas. Compteur changé entre l'appui et le relâchement : ce n'était pas un
//!    rectangle ;
//! 3. au relâchement, le rectangle fait au moins `MIN_SELECTION_SIDE` points de large
//!    et de haut.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Mutex;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventType, NSPasteboard, NSPasteboardNameDrag};
use objc2_core_graphics::{CGWindowLevelForKey, CGWindowLevelKey};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, ns_string};
use tidy_core::geometry::{Point, Rect};

use super::screens::main_height;
use crate::RectCallback;
use crate::convert::{MIN_SELECTION_SIDE, selection_rect};

/// `kCGWindowListOptionOnScreenOnly`.
const ON_SCREEN_ONLY: u32 = 1 << 0;
/// `kCGWindowListExcludeDesktopElements`.
const EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    /// Rend un `CFArrayRef` de `CFDictionaryRef` (+1), ou nul.
    fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> *mut c_void;
}

/// Appui retenu sur le bureau.
struct Press {
    start: Point,
    /// Compteur du presse-papiers de glisser à l'appui.
    drag_count: isize,
}

static PRESS: Mutex<Option<Press>> = Mutex::new(None);

fn mouse() -> Point {
    let p = NSEvent::mouseLocation();
    Point {
        x: p.x as f32,
        y: (main_height() - p.y) as f32,
    }
}

fn drag_count() -> isize {
    // SAFETY: constante d'AppKit, toujours définie.
    let name = unsafe { NSPasteboardNameDrag };
    NSPasteboard::pasteboardWithName(name).changeCount()
}

/// Nombre d'un dictionnaire de fenêtre.
fn number(dict: &NSDictionary<NSString, AnyObject>, key: &NSString) -> Option<f64> {
    let value = dict.objectForKey(key)?;
    value.downcast::<NSNumber>().ok().map(|n| n.doubleValue())
}

/// Cadre (`kCGWindowBounds`, coordonnées globales en points, y vers le bas).
fn bounds(dict: &NSDictionary<NSString, AnyObject>) -> Option<Rect> {
    let value = dict.objectForKey(ns_string!("kCGWindowBounds"))?;
    let b = value.downcast::<NSDictionary>().ok()?;
    // SAFETY: le dictionnaire des bornes a des clés NSString et des valeurs NSNumber.
    let b: &NSDictionary<NSString, AnyObject> = unsafe { &*(&*b as *const NSDictionary).cast() };
    Some(Rect::new(
        number(b, ns_string!("X"))? as f32,
        number(b, ns_string!("Y"))? as f32,
        number(b, ns_string!("Width"))? as f32,
        number(b, ns_string!("Height"))? as f32,
    ))
}

/// Le point tombe-t-il sur le bureau, sous toutes les fenêtres (applis, Dock, barre
/// des menus) ? Les fenêtres au niveau du bureau (fond, icônes du Finder, boîtes de
/// Tidy) ne comptent pas.
fn on_desktop(p: Point) -> bool {
    // SAFETY: fonction de CoreGraphics sans précondition ; le résultat est un CFArray
    // (pont gratuit avec NSArray) que nous possédons (+1) : `Retained` le relâchera.
    let list = unsafe {
        let raw = CGWindowListCopyWindowInfo(ON_SCREEN_ONLY | EXCLUDE_DESKTOP_ELEMENTS, 0);
        Retained::from_raw(raw.cast::<NSArray<NSDictionary<NSString, AnyObject>>>())
    };
    let Some(list) = list else {
        // Liste illisible : mieux vaut ne rien proposer.
        return false;
    };
    let icons = CGWindowLevelForKey(CGWindowLevelKey::DesktopIconWindowLevelKey) as f64;
    let blocking = list.iter().any(|w| {
        let layer = number(&w, ns_string!("kCGWindowLayer")).unwrap_or(0.0);
        let alpha = number(&w, ns_string!("kCGWindowAlpha")).unwrap_or(1.0);
        layer > icons + 1.0 && alpha > 0.0 && bounds(&w).is_some_and(|r| r.contains(p))
    });
    !blocking
}

fn on_down() {
    let start = mouse();
    let press = on_desktop(start).then(|| Press {
        start,
        drag_count: drag_count(),
    });
    *PRESS.lock().unwrap_or_else(|p| p.into_inner()) = press;
}

fn on_up(on_rect: &RectCallback) {
    let Some(press) = PRESS.lock().unwrap_or_else(|p| p.into_inner()).take() else {
        return;
    };
    // Une icône a été glissée (le Finder a rempli le presse-papiers de glisser).
    if drag_count() != press.drag_count {
        return;
    }
    if let Some(rect) = selection_rect(press.start, mouse(), MIN_SELECTION_SIDE) {
        on_rect(rect);
    }
}

pub fn watch_drag(on_rect: RectCallback) -> bool {
    if MainThreadMarker::new().is_none() {
        log::warn!("rectangle sur le bureau : pas sur le thread principal");
        return false;
    }
    let block = RcBlock::new(move |event: NonNull<NSEvent>| {
        // SAFETY: AppKit passe un événement valide pendant l'appel du bloc.
        let event = unsafe { event.as_ref() };
        match event.r#type() {
            NSEventType::LeftMouseDown => on_down(),
            NSEventType::LeftMouseUp => on_up(&on_rect),
            _ => {}
        }
    });
    let monitor = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
        NSEventMask::LeftMouseDown | NSEventMask::LeftMouseUp,
        &block,
    );
    match monitor {
        Some(monitor) => {
            // Le moniteur vit jusqu'à la fin du processus.
            std::mem::forget(monitor);
            true
        }
        None => {
            log::warn!("rectangle sur le bureau : moniteur d'événements refusé");
            false
        }
    }
}
