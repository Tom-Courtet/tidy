//! Processus du bureau (`tidy --desktop`) : une surface transparente par écran, posée
//! juste au-dessus des icônes du bureau, sur laquelle Tidy dessine boîtes, icônes et
//! widgets.
//!
//! Vague 0 : prototype. Une boîte de démonstration, déplaçable d'un écran à l'autre,
//! dont la position est mémorisée par configuration d'écrans. Le reste (vraies boîtes,
//! icônes, glisser-déposer…) arrive en vague 1 sur cette même base.
//!
//! - **Mode superposé** : la surface ne prend les clics que sur les boîtes ; ailleurs,
//!   ils passent aux icônes du Finder ([`tidy_platform::DesktopPlatform::set_interactive_regions`]).
//! - **Mode géré** : la surface prend tout le bureau ; un double-clic sur une zone vide
//!   masque ou montre le bureau.
//!
//! Le processus suit l'agent : s'il disparaît (fin de l'abonnement aux événements), le
//! bureau rend les icônes du Finder s'il le faut, puis s'arrête.

mod surface;

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, OnceLock};

use anyhow::Result;
use egui::ViewportId;
use tidy_core::events::Event;
use tidy_core::geometry::Rect;
use tidy_core::model::DeskBox;
use tidy_core::screen::Screen;
use tidy_core::settings::DesktopMode;
use tidy_core::{DesktopState, Settings, Store};
use tidy_platform::SurfaceLevel;
use tidy_platform::finder::{self, SystemFinder};
use tidy_ui::t;

use crate::ipc::{self, Request};

/// Taille de la boîte de démonstration, en points.
const DEMO_SIZE: [f32; 2] = [320.0, 220.0];

pub fn run(store: Store) -> Result<()> {
    let settings: Settings = store.load();
    crate::lang::apply(settings.language);
    let screens = tidy_platform::current().screens();
    anyhow::ensure!(!screens.is_empty(), "aucun écran");

    // Suivre l'agent. S'il disparaît : rendre le Finder et s'arrêter. Chaque événement
    // réveille l'interface (qui, sinon, ne se redessine que sur une entrée).
    let (tx, events) = mpsc::channel();
    let end_store = store.clone();
    let ctx_cell: Arc<OnceLock<egui::Context>> = Arc::default();
    let wake = Arc::clone(&ctx_cell);
    if let Err(e) = ipc::subscribe(
        &store,
        move |event| {
            let _ = tx.send(event);
            if let Some(ctx) = wake.get() {
                ctx.request_repaint();
            }
        },
        move || {
            log::warn!("l'agent a disparu : le bureau s'arrête");
            let _ = finder::restore_icons(&SystemFinder, &end_store.finder_marker_path());
            std::process::exit(0);
        },
    ) {
        // Lancé à la main, sans agent : on continue (prototype, essais).
        log::warn!("pas d'agent ({e:#}) : bureau autonome");
    }

    let root = &screens[0];
    let viewport = surface::builder(root, &surface::title(0));
    let native = eframe::NativeOptions {
        viewport,
        persist_window: false,
        event_loop_builder: Some(Box::new(|builder| {
            #[cfg(target_os = "macos")]
            {
                use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
                // Ni Dock ni ⌘Tab : le bureau n'est pas une app à part entière.
                builder
                    .with_activation_policy(ActivationPolicy::Accessory)
                    .with_default_menu(false);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = builder;
        })),
        ..Default::default()
    };
    let root_frame = root.frame;
    eframe::run_native(
        "Tidy",
        native,
        Box::new(move |cc| {
            tidy_platform::current().make_desktop_surface(
                cc,
                root_frame,
                SurfaceLevel::DesktopIcons,
            );
            tidy_ui::theme::apply(&cc.egui_ctx);
            let _ = ctx_cell.set(cc.egui_ctx.clone());
            Ok(Box::new(Desktop::new(store, settings, screens, events)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("bureau : {e}"))
}

/// Une surface (un écran).
struct SurfaceState {
    screen: Screen,
    viewport: ViewportId,
    title: String,
    /// Fenêtre native réglée (niveau, Spaces…).
    placed: bool,
    /// Dernières régions interactives envoyées (mode superposé).
    regions: Option<Option<Vec<Rect>>>,
}

struct Desktop {
    store: Store,
    settings: Settings,
    state: DesktopState,
    surfaces: Vec<SurfaceState>,
    events: Receiver<Event>,
    /// Boîte de démonstration : position globale courante.
    demo: Rect,
    hidden: bool,
    ready_sent: bool,
}

impl Desktop {
    fn new(
        store: Store,
        settings: Settings,
        screens: Vec<Screen>,
        events: Receiver<Event>,
    ) -> Self {
        let mut state: DesktopState = store.load();
        if state.boxes.is_empty() {
            state.boxes.push(DeskBox::new(t!("desktop.demo_box")));
        }
        let demo = state.boxes[0]
            .placement
            .resolve(&screens)
            .map(|(_, r)| r)
            .unwrap_or_else(|| default_demo_rect(&screens[0]));
        let surfaces = screens
            .into_iter()
            .enumerate()
            .map(|(i, screen)| SurfaceState {
                screen,
                viewport: if i == 0 {
                    ViewportId::ROOT
                } else {
                    ViewportId::from_hash_of(("tidy-surface", i))
                },
                title: surface::title(i),
                placed: i == 0,
                regions: None,
            })
            .collect();
        Self {
            store,
            hidden: false,
            settings,
            state,
            surfaces,
            events,
            demo,
            ready_sent: false,
        }
    }

    fn mode(&self) -> DesktopMode {
        self.settings.effective_desktop_mode()
    }

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::DesktopVisibility { hidden } => self.hidden = hidden,
                Event::SettingsChanged => {
                    self.settings = self.store.load();
                    crate::lang::apply(self.settings.language);
                    for s in &mut self.surfaces {
                        s.regions = None;
                    }
                }
                Event::Shutdown => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                _ => {}
            }
        }
    }

    /// Enregistre la position de la boîte de démonstration après un déplacement.
    fn remember_demo(&mut self) {
        let screens: Vec<Screen> = self.surfaces.iter().map(|s| s.screen.clone()).collect();
        let center = tidy_core::geometry::Point {
            x: self.demo.x + self.demo.width / 2.0,
            y: self.demo.y + self.demo.height / 2.0,
        };
        let Some(screen) = screens
            .iter()
            .find(|s| s.frame.contains(center))
            .or_else(|| screens.first())
        else {
            return;
        };
        self.demo = self.demo.clamped_to(&screen.visible_frame);
        self.state.boxes[0]
            .placement
            .remember(&screens, screen, self.demo);
        if let Err(e) = self.store.save(&self.state) {
            log::error!("disposition non enregistrée : {e:#}");
        }
    }

    /// Régions interactives d'une surface (mode superposé) : les boîtes visibles.
    fn regions_for(&self, screen: &Screen) -> Option<Vec<Rect>> {
        match self.mode() {
            DesktopMode::Managed => None,
            _ if self.hidden => Some(Vec::new()),
            _ => Some(
                [self.demo]
                    .into_iter()
                    .filter(|r| r.intersects(&screen.frame))
                    .collect(),
            ),
        }
    }

    fn sync_regions(&mut self, index: usize) {
        let wanted = self.regions_for(&self.surfaces[index].screen);
        let surface = &mut self.surfaces[index];
        if surface.regions.as_ref() != Some(&wanted) {
            tidy_platform::current().set_interactive_regions(&surface.title, wanted.clone());
            surface.regions = Some(wanted);
        }
    }

    fn surface_ui(&mut self, ui: &mut egui::Ui, index: usize) {
        let origin = self.surfaces[index].screen.frame.origin();
        let out = surface::draw(
            ui,
            surface::Frame {
                origin,
                demo: self.demo,
                hidden: self.hidden,
                managed: self.mode() == DesktopMode::Managed,
                title: self.state.boxes[0].name.clone(),
            },
        );
        if let Some(delta) = out.dragged {
            self.demo.x += delta.x;
            self.demo.y += delta.y;
        }
        if out.drag_stopped {
            self.remember_demo();
        }
        if out.background_double_clicked {
            // Passe par l'agent, qui tient l'état et prévient tout le monde.
            std::thread::spawn({
                let store = self.store.clone();
                move || {
                    let _ = ipc::request(&store, &Request::ToggleDesktop);
                }
            });
        }
        self.sync_regions(index);
    }
}

fn default_demo_rect(screen: &Screen) -> Rect {
    let v = screen.visible_frame;
    Rect::new(
        v.right() - DEMO_SIZE[0] - 40.0,
        v.y + 40.0,
        DEMO_SIZE[0],
        DEMO_SIZE[1],
    )
}

impl eframe::App for Desktop {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_events(&ctx);
        self.surface_ui(ui, 0);
        for i in 1..self.surfaces.len() {
            let builder = surface::builder(&self.surfaces[i].screen, &self.surfaces[i].title);
            let viewport = self.surfaces[i].viewport;
            ctx.show_viewport_immediate(viewport, builder, |ui, _class| self.surface_ui(ui, i));
            if !self.surfaces[i].placed {
                let s = &self.surfaces[i];
                self.surfaces[i].placed = tidy_platform::current().make_desktop_surface_titled(
                    &s.title,
                    s.screen.frame,
                    SurfaceLevel::DesktopIcons,
                );
                ctx.request_repaint();
            }
        }
        if !self.ready_sent && self.surfaces.iter().all(|s| s.placed) {
            self.ready_sent = true;
            let store = self.store.clone();
            std::thread::spawn(move || {
                let _ = ipc::request(&store, &Request::DesktopReady);
            });
        }
    }
}
