//! Lanceur de recherche rapide (`tidy --search [texte]`, lot G) : fenêtre ouverte à la
//! demande par l'agent (raccourci) ou par le widget « Search Mini ».
//!
//! **Processus** : lancé par l'agent (variable [`CHILD_ENV`]), il reste en mémoire
//! après la première ouverture, fenêtre cachée, et se rouvre aussitôt quand l'agent
//! relaie `search/show` (réglage « Garder en mémoire », activé par défaut). Son index
//! est tenu à jour en arrière-plan. Il s'arrête avec l'agent. Lancé à la main, il passe
//! par l'agent s'il tourne (une seule fenêtre), sinon s'ouvre seul et se ferme avec sa
//! fenêtre.
//!
//! L'agent ne porte pas l'index : il reste léger.

mod icons;
mod launcher;

use std::sync::mpsc;
use std::sync::{Arc, OnceLock};

use anyhow::Result;
use serde_json::json;
use tidy_core::Store;
use tidy_core::events::Event;
use tidy_core::geometry::Point;
use tidy_core::screen::Screen;
use tidy_core::settings::Settings;
use tidy_search::{Config, Engine};

use crate::agent::search::CHILD_ENV;
use crate::ipc::{self, Request, Response};
pub use launcher::{TITLE, tab_label};

/// Taille de la fenêtre, en points.
const SIZE: [f32; 2] = [720.0, 480.0];

/// Ouvre le lanceur, avec `query` déjà saisi.
pub fn run(store: Store, query: Option<String>) -> Result<()> {
    let settings: Settings = store.load();
    crate::lang::apply(settings.language);
    let from_agent = std::env::var_os(CHILD_ENV).is_some();
    if !from_agent {
        // Lancé à la main : l'agent ouvre (ou ramène) son lanceur.
        let req = Request::Module {
            module: "search".into(),
            action: "open".into(),
            data: json!({ "query": query }),
        };
        if matches!(ipc::request(&store, &req), Ok(Response::Ok)) {
            return Ok(());
        }
        log::info!("pas d'agent : lanceur de recherche seul");
    }
    let resident = from_agent && settings.search.keep_in_background;

    let ctx_cell: Arc<OnceLock<egui::Context>> = Arc::default();
    let (tx, events) = mpsc::channel();
    if from_agent {
        let wake = Arc::clone(&ctx_cell);
        let subscribed = ipc::subscribe(
            &store,
            move |event| {
                if event == Event::Shutdown {
                    std::process::exit(0);
                }
                let _ = tx.send(event);
                if let Some(ctx) = wake.get() {
                    ctx.request_repaint();
                }
            },
            || {
                log::info!("l'agent s'est arrêté : le lanceur aussi");
                std::process::exit(0);
            },
        );
        if let Err(e) = subscribed {
            log::warn!("lanceur sans événements de l'agent : {e:#}");
        }
    }

    let wake = Arc::clone(&ctx_cell);
    let engine = Engine::start(
        Config::from_settings(&settings.search),
        Some(tidy_search::cache_path(store.root())),
        move || {
            if let Some(ctx) = wake.get() {
                ctx.request_repaint();
            }
        },
    );

    let platform = tidy_platform::current();
    let pos = placement(&platform.screens(), platform.mouse_location(), SIZE);
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_position(pos)
            .with_inner_size(SIZE),
        persist_window: false,
        event_loop_builder: Some(Box::new(|builder| {
            #[cfg(target_os = "macos")]
            {
                use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
                // Ni Dock ni ⌘Tab : le lanceur est une fenêtre de passage.
                builder
                    .with_activation_policy(ActivationPolicy::Accessory)
                    .with_default_menu(false);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = builder;
        })),
        ..Default::default()
    };
    eframe::run_native(
        TITLE,
        native,
        Box::new(move |cc| {
            tidy_ui::theme::apply(&cc.egui_ctx);
            let _ = ctx_cell.set(cc.egui_ctx.clone());
            Ok(Box::new(launcher::Launcher::new(
                store,
                settings,
                engine,
                events,
                resident,
                query,
                cc.egui_ctx.clone(),
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("lanceur de recherche : {e}"))
}

/// Position de la fenêtre : centrée en largeur sur l'écran de la souris, au premier
/// cinquième de sa hauteur utile.
pub fn placement(screens: &[Screen], mouse: Point, size: [f32; 2]) -> [f32; 2] {
    let screen = screens
        .iter()
        .find(|s| s.frame.contains(mouse))
        .or_else(|| screens.iter().find(|s| s.primary))
        .or(screens.first());
    let Some(screen) = screen else {
        return [100.0, 100.0];
    };
    let v = screen.visible_frame;
    let x = v.x + (v.width - size[0]) / 2.0;
    let y = (v.y + v.height * 0.2).min(v.bottom() - size[1]);
    [x.max(v.x), y.max(v.y)]
}

#[cfg(test)]
mod tests {
    use tidy_core::geometry::Rect;
    use tidy_core::screen::ScreenId;

    use super::*;

    fn screen(x: f32, primary: bool) -> Screen {
        Screen {
            id: ScreenId(format!("{x}")),
            name: String::new(),
            frame: Rect::new(x, 0.0, 1920.0, 1080.0),
            visible_frame: Rect::new(x, 0.0, 1920.0, 1040.0),
            scale: 1.0,
            primary,
        }
    }

    #[test]
    fn placed_on_the_mouse_screen() {
        let screens = [screen(0.0, true), screen(1920.0, false)];
        let pos = placement(
            &screens,
            Point {
                x: 2500.0,
                y: 300.0,
            },
            [720.0, 480.0],
        );
        assert_eq!(pos, [1920.0 + 600.0, 208.0]);
        // Souris hors de tout écran : l'écran principal.
        let pos = placement(&screens, Point { x: -50.0, y: 0.0 }, [720.0, 480.0]);
        assert_eq!(pos[0], 600.0);
        assert_eq!(
            placement(&[], Point { x: 0.0, y: 0.0 }, SIZE),
            [100.0, 100.0]
        );
    }
}
