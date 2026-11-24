//! Changements d'écrans : `NSApplicationDidChangeScreenParametersNotification`
//! (branchement, résolution, échelle, Dock ou barre des menus qui change le cadre
//! utile), reçue sur la file principale.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::NSApplicationDidChangeScreenParametersNotification;
use objc2_foundation::{NSNotification, NSNotificationCenter, NSOperationQueue};

use crate::Callback;

pub fn watch_screens(on_change: Callback) -> bool {
    if MainThreadMarker::new().is_none() {
        log::warn!("surveillance des écrans : pas sur le thread principal");
        return false;
    }
    let block = RcBlock::new(move |_notification: NonNull<NSNotification>| on_change());
    // SAFETY: nom de notification d'AppKit, toujours défini ; pas d'objet filtré ; file
    // principale (le rappel est appelé sur le thread principal) ; le bloc ne capture
    // qu'un rappel `Send + Sync`.
    let observer = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSApplicationDidChangeScreenParametersNotification),
            None,
            Some(&NSOperationQueue::mainQueue()),
            &block,
        )
    };
    // L'observateur vit jusqu'à la fin du processus.
    std::mem::forget(observer);
    true
}
