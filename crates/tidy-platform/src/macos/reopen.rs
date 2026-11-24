//! macOS : Tidy rouvert depuis le Finder, le Launchpad ou Spotlight alors qu'il
//! tourne déjà. macOS ne relance pas l'app : il envoie l'événement Apple « rouvrir »
//! (`aevt`/`rapp`) à l'instance en cours. On y répond en ouvrant la fenêtre
//! principale. Repris de Snappy.

use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{AllocAnyThread, class, define_class, msg_send, sel};

type OnReopen = Box<dyn Fn() + Send + Sync>;

static ON_REOPEN: OnceLock<OnReopen> = OnceLock::new();

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "TidyReopenHandler"]
    struct ReopenHandler;

    impl ReopenHandler {
        #[unsafe(method(handleReopen:withReplyEvent:))]
        fn handle_reopen(&self, _event: *mut AnyObject, _reply: *mut AnyObject) {
            if let Some(on_reopen) = ON_REOPEN.get() {
                on_reopen();
            }
        }
    }
);

/// Installe la réponse à « rouvrir ». Une seule fois, sur le thread principal, une
/// fois la boucle d'événements lancée (après le gestionnaire par défaut d'AppKit).
pub fn install(on_reopen: impl Fn() + Send + Sync + 'static) {
    if ON_REOPEN.set(Box::new(on_reopen)).is_err() {
        return;
    }
    // SAFETY: `init` de NSObject, sur une instance tout juste allouée.
    let handler: Retained<ReopenHandler> = unsafe { msg_send![ReopenHandler::alloc(), init] };
    // SAFETY: NSAppleEventManager existe dans Foundation ; le résultat est vérifié.
    let manager: *mut AnyObject =
        unsafe { msg_send![class!(NSAppleEventManager), sharedAppleEventManager] };
    if manager.is_null() {
        return;
    }
    let event_class = u32::from_be_bytes(*b"aevt");
    let event_id = u32::from_be_bytes(*b"rapp");
    // SAFETY: le gestionnaire répond au sélecteur donné, avec la signature attendue
    // (`handleEvent:withReplyEvent:`) ; il vit jusqu'à la fin du processus.
    unsafe {
        let _: () = msg_send![
            manager,
            setEventHandler: &*handler,
            andSelector: sel!(handleReopen:withReplyEvent:),
            forEventClass: event_class,
            andEventID: event_id
        ];
    }
    std::mem::forget(handler);
}
