//! Mode superposé : la surface du bureau ne prend les clics que sur les boîtes.
//!
//! Une fenêtre macOS reçoit tous les clics ou aucun (`ignoresMouseEvents`). On bascule
//! donc ce réglage selon la position de la souris, suivie par deux moniteurs
//! d'événements : global (souris au-dessus des autres applis, y compris le bureau du
//! Finder) et local (souris au-dessus de nos fenêtres). Aucun minuteur : rien ne
//! tourne tant que la souris ne bouge pas. Pendant un glisser (bouton enfoncé), l'état
//! ne change pas, pour ne pas lâcher une boîte en cours de déplacement.
//!
//! Les moniteurs globaux de mouvements de souris ne demandent pas l'autorisation
//! Accessibilité.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::NonNull;
use std::sync::Mutex;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSApplication, NSEvent, NSEventMask};
use tidy_core::geometry::{Point, Rect};

/// Régions interactives par titre de surface (`None` : toute la surface).
static REGIONS: Mutex<Option<HashMap<String, Option<Vec<Rect>>>>> = Mutex::new(None);

thread_local! {
    /// Moniteurs installés (gardés en vie pour toute la durée du processus).
    static MONITORS: RefCell<Vec<Retained<AnyObject>>> = const { RefCell::new(Vec::new()) };
}

pub fn set_regions(title: &str, regions: Option<Vec<Rect>>) {
    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("régions interactives : pas sur le thread principal");
        return;
    };
    {
        let mut map = REGIONS.lock().unwrap_or_else(|p| p.into_inner());
        map.get_or_insert_with(HashMap::new)
            .insert(title.to_owned(), regions);
    }
    install_monitors();
    update(mtm);
}

fn install_monitors() {
    MONITORS.with(|monitors| {
        let mut monitors = monitors.borrow_mut();
        if !monitors.is_empty() {
            return;
        }
        let mask = NSEventMask::MouseMoved | NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp;
        let global = RcBlock::new(|_event: NonNull<NSEvent>| {
            if let Some(mtm) = MainThreadMarker::new() {
                update(mtm);
            }
        });
        if let Some(m) = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &global) {
            monitors.push(m);
        }
        let local = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
            if let Some(mtm) = MainThreadMarker::new() {
                update(mtm);
            }
            event.as_ptr()
        });
        // SAFETY : le bloc rend toujours l'événement reçu, pointeur valide.
        if let Some(m) = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &local) } {
            monitors.push(m);
        }
    });
}

/// Doit-on laisser passer les clics au point `p` ?
fn should_ignore(regions: &Option<Vec<Rect>>, p: Point) -> bool {
    match regions {
        None => false,
        Some(rects) => !rects.iter().any(|r| r.contains(p)),
    }
}

fn update(mtm: MainThreadMarker) {
    if NSEvent::pressedMouseButtons() != 0 {
        return;
    }
    let p = NSEvent::mouseLocation();
    let point = Point {
        x: p.x as f32,
        y: (super::screens::main_height() - p.y) as f32,
    };
    let map = REGIONS.lock().unwrap_or_else(|p| p.into_inner());
    let Some(map) = map.as_ref() else {
        return;
    };
    for win in NSApplication::sharedApplication(mtm).windows().iter() {
        let title = win.title().to_string();
        if let Some(regions) = map.get(&title) {
            let ignore = should_ignore(regions, point);
            if win.ignoresMouseEvents() != ignore {
                win.setIgnoresMouseEvents(ignore);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_pass_through_outside_regions() {
        let p = Point { x: 50.0, y: 50.0 };
        assert!(!should_ignore(&None, p));
        assert!(should_ignore(&Some(vec![]), p));
        assert!(!should_ignore(&Some(vec![Rect::new(0.0, 0.0, 100.0, 100.0)]), p));
        assert!(should_ignore(&Some(vec![Rect::new(60.0, 0.0, 100.0, 100.0)]), p));
    }
}
