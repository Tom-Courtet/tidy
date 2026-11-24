//! Fichiers vus par le Finder : icônes, glisser-déposer vers une autre application.
//!
//! - Icône : `NSWorkspace iconForFile:` (l'icône du Finder, avec les aperçus de
//!   documents que macOS y met lui-même), rendue dans un bitmap CoreGraphics à la
//!   taille demandée. Pas de miniature QuickLook : asynchrone, laissée pour plus tard.
//! - Glisser : session AppKit (`beginDraggingSessionWithItems`) avec une URL de fichier
//!   par élément, comme Snappy (crate `drag`), sans dépendance de plus.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession,
    NSDraggingSource, NSEvent, NSEventModifierFlags, NSEventType, NSImage, NSWindow, NSWorkspace,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo, CGInterpolationQuality,
    kCGColorSpaceSRGB,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString, NSURL};

use super::window::find_titled;
use crate::IconImage;
use crate::convert::{icon_side, unpremultiply_rgba};

/// Côté de l'image montrée sous le curseur pendant un glisser, en points.
const DRAG_IMAGE_SIDE: f64 = 64.0;
/// Décalage entre les images de plusieurs éléments glissés, en points.
const DRAG_STACK_OFFSET: f64 = 6.0;
/// Au-delà, les éléments suivants partagent la position du dernier.
const DRAG_STACK_MAX: usize = 4;

fn icon_for(path: &Path) -> Retained<NSImage> {
    NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(&path.to_string_lossy()))
}

/// Icône du Finder, en RGBA non prémultiplié, `size_px` pixels de côté.
pub fn file_icon(path: &Path, size_px: u32) -> Result<IconImage> {
    let side = icon_side(size_px) as usize;
    let image = icon_for(path);
    let mut proposed = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(side as f64, side as f64),
    );
    // SAFETY: `proposed` est un rectangle local valide ; ni contexte ni indications.
    let cg = unsafe { image.CGImageForProposedRect_context_hints(&mut proposed, None, None) }
        .context("icône sans image")?;
    // SAFETY: constante de CoreGraphics, toujours définie.
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
        .context("espace de couleurs sRGB indisponible")?;
    let mut rgba = vec![0u8; side * side * 4];
    // SAFETY: `rgba` a la place de `side` lignes de `side * 4` octets et survit au
    // contexte (relâché avant la lecture des pixels).
    let context = unsafe {
        CGBitmapContextCreate(
            rgba.as_mut_ptr().cast(),
            side,
            side,
            8,
            side * 4,
            Some(&space),
            // 8 bits par composante, ordre par défaut : octets R, G, B, A.
            CGImageAlphaInfo::PremultipliedLast.0,
        )
    }
    .context("bitmap impossible")?;
    CGContext::set_interpolation_quality(Some(&context), CGInterpolationQuality::High);
    CGContext::draw_image(
        Some(&context),
        CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(side as f64, side as f64),
        ),
        Some(&cg),
    );
    drop(context);
    unpremultiply_rgba(&mut rgba);
    Ok(IconImage {
        width: side as u32,
        height: side as u32,
        rgba,
    })
}

// --- Glisser-déposer ------------------------------------------------------------

struct DragSourceIvars {
    window: Retained<NSWindow>,
}

define_class!(
    // SAFETY: NSObject n'a pas d'exigence de sous-classe ; pas de Drop personnalisé.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TidyFileDragSource"]
    #[ivars = DragSourceIvars]
    struct DragSource;

    // SAFETY: NSObjectProtocol n'a pas de méthode obligatoire.
    unsafe impl NSObjectProtocol for DragSource {}

    // SAFETY: signatures conformes au protocole NSDraggingSource.
    unsafe impl NSDraggingSource for DragSource {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn operation_mask(
            &self,
            _session: &NSDraggingSession,
            _context: NSDraggingContext,
        ) -> NSDragOperation {
            // La cible choisit (le Finder déplace sur le même disque, copie sinon).
            NSDragOperation::Copy
                | NSDragOperation::Move
                | NSDragOperation::Link
                | NSDragOperation::Generic
        }

        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn ended(
            &self,
            _session: &NSDraggingSession,
            screen_point: NSPoint,
            _operation: NSDragOperation,
        ) {
            release_button(&self.ivars().window, screen_point);
        }
    }
);

impl DragSource {
    fn new(window: Retained<NSWindow>, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DragSourceIvars { window });
        // SAFETY: `init` de NSObject sur une instance tout juste allouée.
        unsafe { msg_send![super(this), init] }
    }
}

/// La session AppKit garde le relâchement du bouton pour elle : on le rend à la
/// fenêtre, sinon winit (et egui) croiraient le bouton toujours enfoncé.
fn release_button(window: &NSWindow, screen_point: NSPoint) {
    let mtm = MainThreadMarker::from(window);
    let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        NSEventType::LeftMouseUp,
        window.convertPointFromScreen(screen_point),
        NSEventModifierFlags::empty(),
        NSApplication::sharedApplication(mtm)
            .currentEvent()
            .map_or(0.0, |e| e.timestamp()),
        window.windowNumber(),
        None,
        0,
        1,
        0.0,
    );
    match event {
        Some(event) => NSApplication::sharedApplication(mtm).postEvent_atStart(&event, false),
        None => log::debug!("glisser : relâchement du bouton impossible à signaler"),
    }
}

/// Glisser-déposer de ces fichiers depuis la fenêtre `window_title`. Rend la main
/// tout de suite : la session continue dans la boucle d'AppKit.
pub fn start_drag(window_title: &str, paths: &[PathBuf]) -> Result<bool> {
    if paths.is_empty() {
        return Ok(false);
    }
    let Some(mtm) = MainThreadMarker::new() else {
        bail!("glisser : pas sur le thread principal");
    };
    let window = find_titled(mtm, window_title)
        .with_context(|| format!("fenêtre « {window_title} » introuvable"))?;
    let view = window.contentView().context("fenêtre sans contenu")?;
    let in_window = window.mouseLocationOutsideOfEventStream();
    let in_view = view.convertPoint_fromView(in_window, None);

    let mut items: Vec<Retained<NSDraggingItem>> = Vec::with_capacity(paths.len());
    for (i, path) in paths.iter().enumerate() {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let item = NSDraggingItem::initWithPasteboardWriter(
            NSDraggingItem::alloc(),
            &ProtocolObject::from_retained(url),
        );
        let icon = icon_for(path);
        icon.setSize(NSSize::new(DRAG_IMAGE_SIDE, DRAG_IMAGE_SIDE));
        let offset = i.min(DRAG_STACK_MAX) as f64 * DRAG_STACK_OFFSET;
        let frame = NSRect::new(
            NSPoint::new(
                in_view.x - DRAG_IMAGE_SIDE / 2.0 + offset,
                in_view.y - DRAG_IMAGE_SIDE / 2.0 + offset,
            ),
            NSSize::new(DRAG_IMAGE_SIDE, DRAG_IMAGE_SIDE),
        );
        // SAFETY: le contenu est une NSImage, type attendu par AppKit.
        unsafe { item.setDraggingFrame_contents(frame, Some(&icon)) };
        items.push(item);
    }
    let items = NSArray::from_retained_slice(&items);

    // L'événement souris en cours (glissé) si c'en est un, sinon un glissé fabriqué à
    // la position de la souris.
    let app = NSApplication::sharedApplication(mtm);
    let current = app.currentEvent().filter(|e| {
        matches!(
            e.r#type(),
            NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged
        )
    });
    let event = match current {
        Some(event) => event,
        None => NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            NSEventType::LeftMouseDragged,
            in_window,
            NSEventModifierFlags::empty(),
            app.currentEvent().map_or(0.0, |e| e.timestamp()),
            window.windowNumber(),
            None,
            0,
            1,
            1.0,
        )
        .context("événement de glisser impossible")?,
    };
    let source = DragSource::new(window.clone(), mtm);
    view.beginDraggingSessionWithItems_event_source(
        &items,
        &event,
        ProtocolObject::from_ref(&*source),
    );
    // La source doit vivre jusqu'à la fin de la session : gardée jusqu'au glisser
    // suivant.
    LAST_SOURCE.with(|last| *last.borrow_mut() = Some(source));
    Ok(true)
}

thread_local! {
    /// Source du dernier glisser (thread principal).
    static LAST_SOURCE: std::cell::RefCell<Option<Retained<DragSource>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finder_icon_has_the_requested_size() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, "x").unwrap();
        let icon = file_icon(&file, 48).unwrap();
        assert_eq!((icon.width, icon.height), (48, 48));
        assert_eq!(icon.rgba.len(), 48 * 48 * 4);
        // L'icône n'est pas vide : des pixels opaques.
        assert!(icon.rgba.as_chunks::<4>().0.iter().any(|px| px[3] == 255));
    }
}
