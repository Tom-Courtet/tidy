//! Session et veille (lot F, boîte privée) :
//! - veille et réveil, session quittée (changement rapide d'utilisateur) : centre de
//!   notifications de `NSWorkspace` ;
//! - écran verrouillé : notification distribuée `com.apple.screenIsLocked` (non
//!   documentée mais stable depuis des années) ;
//! - inactivité : `CGEventSourceSecondsSinceLastEventType`.

use std::ptr::NonNull;
use std::sync::Arc;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceSessionDidBecomeActiveNotification,
    NSWorkspaceSessionDidResignActiveNotification, NSWorkspaceWillSleepNotification,
};
use objc2_core_graphics::{CGEventSource, CGEventSourceStateID, CGEventType};
use objc2_foundation::{
    NSDistributedNotificationCenter, NSNotification, NSNotificationCenter, NSNotificationName,
    NSOperationQueue, NSString,
};

use crate::{SessionCallback, SessionEvent};

/// Abonne `event` à la notification `name` du centre `center`, sur la file principale.
fn observe(
    center: &NSNotificationCenter,
    name: &NSNotificationName,
    on_event: &Arc<SessionCallback>,
    event: SessionEvent,
) {
    let on_event = Arc::clone(on_event);
    let block = RcBlock::new(move |_n: NonNull<NSNotification>| on_event(event));
    // SAFETY: nom de notification valide ; pas d'objet filtré ; file principale (le
    // rappel est appelé sur le thread principal) ; le bloc ne capture qu'un rappel
    // `Send + Sync`.
    let observer = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(name),
            None,
            Some(&NSOperationQueue::mainQueue()),
            &block,
        )
    };
    // L'observateur vit jusqu'à la fin du processus.
    std::mem::forget(observer);
}

pub fn watch_session(on_event: SessionCallback) -> bool {
    if MainThreadMarker::new().is_none() {
        log::warn!("surveillance de la session : pas sur le thread principal");
        return false;
    }
    let on_event = Arc::new(on_event);
    let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
    // SAFETY: noms de notification d'AppKit, toujours définis.
    let names = unsafe {
        [
            (NSWorkspaceWillSleepNotification, SessionEvent::Sleep),
            (NSWorkspaceDidWakeNotification, SessionEvent::Wake),
            (
                NSWorkspaceSessionDidResignActiveNotification,
                SessionEvent::Locked,
            ),
            (
                NSWorkspaceSessionDidBecomeActiveNotification,
                SessionEvent::Unlocked,
            ),
        ]
    };
    for (name, event) in names {
        observe(&workspace, name, &on_event, event);
    }
    let distributed = NSDistributedNotificationCenter::defaultCenter();
    for (name, event) in [
        ("com.apple.screenIsLocked", SessionEvent::Locked),
        ("com.apple.screenIsUnlocked", SessionEvent::Unlocked),
    ] {
        observe(&distributed, &NSString::from_str(name), &on_event, event);
    }
    true
}

/// Secondes depuis la dernière entrée (clavier, souris, pavé) de la session.
pub fn idle_seconds() -> Option<f64> {
    // `kCGAnyInputEventType` : tous les types d'entrée.
    let any = CGEventType(u32::MAX);
    let s = CGEventSource::seconds_since_last_event_type(
        CGEventSourceStateID::CombinedSessionState,
        any,
    );
    (s.is_finite() && s >= 0.0).then_some(s)
}
