//! Fond « verre dépoli » des boîtes, sous macOS (lot M) : une `NSVisualEffectView`
//! (matériau HUD, sombre, mélangée à ce qui est *derrière* la fenêtre, toujours
//! active) posée sous le contenu d'egui, à la taille de la fenêtre. Là où egui laisse
//! le fond transparent ou à demi transparent, on voit le flou.
//!
//! La vue de contenu de winit a une couche (OpenGL) : une sous-vue la recouvrirait.
//! L'effet est donc posé à côté d'elle, dans la vue du cadre de la fenêtre (sa
//! super-vue), juste en dessous. winit retrouve sa vue par `contentView`, qui ne change
//! pas.
//!
//! La vue est retrouvée par sa classe pour être retirée ; elle suit la taille de la
//! fenêtre (enroulement, redimensionnement) par son masque de redimensionnement.

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSWindowOrderingMode};
use objc2_foundation::NSObjectProtocol;

use super::window;

/// `NSVisualEffectMaterialHUDWindow`.
const MATERIAL_HUD: isize = 13;
/// `NSVisualEffectBlendingModeBehindWindow`.
const BLENDING_BEHIND_WINDOW: isize = 0;
/// `NSVisualEffectStateActive` : flou même quand la fenêtre n'est pas active.
const STATE_ACTIVE: isize = 1;

pub fn set_glass(title: &str, enabled: bool, corner_radius: f32) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("verre : pas sur le thread principal");
        return false;
    };
    let Some(win) = window::find_titled(mtm, title) else {
        log::debug!("verre : fenêtre « {title} » introuvable");
        return false;
    };
    let Some(content) = win.contentView() else {
        return false;
    };
    // SAFETY: lecture de la super-vue sur le thread principal ; la vue du cadre vit
    // avec la fenêtre, que nous tenons.
    let Some(frame_view) = (unsafe { content.superview() }) else {
        return false;
    };
    let Some(class) = AnyClass::get(c"NSVisualEffectView") else {
        return false;
    };
    // Retirer l'effet posé avant (changement de rayon, ou verre retiré).
    for sub in frame_view.subviews().iter() {
        if sub.isKindOfClass(class) {
            sub.removeFromSuperview();
        }
    }
    if !enabled {
        return false;
    }
    let frame = content.frame();
    // SAFETY: NSVisualEffectView est une sous-classe de NSView ; `alloc` puis
    // `initWithFrame:` (initialiseur désigné de NSView) avec un NSRect.
    let effect: Retained<NSView> = unsafe {
        let allocated: Allocated<NSView> = msg_send![class, alloc];
        msg_send![allocated, initWithFrame: frame]
    };
    // SAFETY: propriétés NSInteger documentées de NSVisualEffectView, valeurs de ses
    // énumérations.
    unsafe {
        let _: () = msg_send![&*effect, setMaterial: MATERIAL_HUD];
        let _: () = msg_send![&*effect, setBlendingMode: BLENDING_BEHIND_WINDOW];
        let _: () = msg_send![&*effect, setState: STATE_ACTIVE];
    }
    effect.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    if corner_radius >= 1.0 {
        effect.setWantsLayer(true);
        // SAFETY: la vue a une couche (`setWantsLayer`) ; `cornerRadius` (CGFloat) et
        // `masksToBounds` (BOOL) sont des propriétés de CALayer.
        unsafe {
            let layer: *mut AnyObject = msg_send![&*effect, layer];
            if let Some(layer) = layer.as_ref() {
                let _: () = msg_send![layer, setCornerRadius: f64::from(corner_radius)];
                let _: () = msg_send![layer, setMasksToBounds: Bool::YES];
            }
        }
    }
    frame_view.addSubview_positioned_relativeTo(
        &effect,
        NSWindowOrderingMode::Below,
        Some(&content),
    );
    true
}
