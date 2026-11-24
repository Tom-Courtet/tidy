//! Processus du bureau (`tidy --desktop`) : **une fenêtre par boîte** (ADR 011), posée
//! juste au-dessus des icônes du bureau, sous toutes les applis.
//!
//! - Une fenêtre « hôte » de 1×1 point, invisible, porte la boucle d'eframe ; chaque
//!   boîte est une fenêtre secondaire (viewport immédiat) à sa taille.
//! - Les événements de l'agent sont traités dans [`eframe::App::logic`], qui tourne
//!   même quand toutes les fenêtres sont cachées derrière des applis ; un changement
//!   de structure (boîte créée, bureau montré) demande une image malgré tout
//!   (`RequestPaintWhileHidden`).
//! - Le système déplace et redimensionne les fenêtres ; la position finale est relue et
//!   enregistrée par configuration d'écrans.
//!
//! Le processus suit l'agent : s'il disparaît (fin de l'abonnement aux événements), le
//! bureau rend les icônes du système s'il le faut, puis s'arrête.

mod boxwin;

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use egui::{ViewportBuilder, ViewportCommand, ViewportId};
use tidy_core::events::Event;
use tidy_core::geometry::{Point, Rect};
use tidy_core::model::{BoxId, DeskBox};
use tidy_core::screen::Screen;
use tidy_core::{DesktopState, Settings, Store};
use tidy_platform::SurfaceLevel;
use tidy_platform::finder::{self, SystemFinder};
use tidy_ui::t;

use crate::ipc::{self, Request};

/// Taille d'une nouvelle boîte, en points.
const NEW_BOX_SIZE: [f32; 2] = [320.0, 220.0];
/// Délai après le dernier mouvement d'une fenêtre avant d'enregistrer sa position.
const SAVE_DELAY: Duration = Duration::from_millis(400);

pub fn run(store: Store) -> Result<()> {
    let settings: Settings = store.load();
    crate::lang::apply(settings.language);
    let screens = tidy_platform::current().screens();
    anyhow::ensure!(!screens.is_empty(), "aucun écran");

    // Suivre l'agent. Chaque événement réveille la boucle (`logic`).
    let (tx, events) = mpsc::channel();
    let ctx_cell: Arc<OnceLock<egui::Context>> = Arc::default();
    let wake = Arc::clone(&ctx_cell);
    let end_store = store.clone();
    if let Err(e) = ipc::subscribe(
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
        move || {
            log::warn!("l'agent a disparu : le bureau rend les icônes et s'arrête");
            restore_system_icons(&end_store);
            std::process::exit(0);
        },
    ) {
        // Lancé à la main, sans agent : on continue (essais).
        log::warn!("pas d'agent ({e:#}) : bureau autonome");
    }

    let host = host_rect(&screens);
    let native = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("Tidy · hôte du bureau")
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_has_shadow(false)
            .with_taskbar(false)
            .with_position([host.x, host.y])
            .with_inner_size([host.width, host.height]),
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
    eframe::run_native(
        "Tidy",
        native,
        Box::new(move |cc| {
            tidy_platform::current().make_desktop_surface(cc, host, SurfaceLevel::DesktopIcons);
            tidy_ui::theme::apply(&cc.egui_ctx);
            let _ = ctx_cell.set(cc.egui_ctx.clone());
            Ok(Box::new(Desktop::new(store, settings, screens, events)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("bureau : {e}"))
}

/// Rend les icônes du bureau du système si Tidy les avait masquées.
pub fn restore_system_icons(store: &Store) {
    let platform = tidy_platform::current();
    if platform.desktop_icons_hidden() {
        let _ = platform.set_desktop_icons_hidden(false);
    }
    let _ = finder::restore_icons(&SystemFinder, &store.finder_marker_path());
    crate::icons::restore_all(store);
}

/// Fenêtre hôte : 1×1 point, au coin du cadre utile de l'écran principal.
fn host_rect(screens: &[Screen]) -> Rect {
    let v = screens
        .iter()
        .find(|s| s.primary)
        .unwrap_or(&screens[0])
        .visible_frame;
    Rect::new(v.x, v.y, 1.0, 1.0)
}

/// État d'une fenêtre de boîte.
struct BoxWindow {
    viewport: ViewportId,
    title: String,
    /// Cadre de création (constant, pour qu'egui ne replace pas la fenêtre).
    spawn: Rect,
    /// Cadre actuel, relu sur la fenêtre.
    current: Rect,
    /// Fenêtre native réglée (niveau, bureau…).
    placed: bool,
    /// Déplacée ou redimensionnée depuis : position à enregistrer.
    moved_at: Option<Instant>,
}

struct Desktop {
    store: Store,
    settings: Settings,
    state: DesktopState,
    screens: Vec<Screen>,
    windows: HashMap<BoxId, BoxWindow>,
    events: Receiver<Event>,
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
        let mut desktop = Self {
            store,
            settings,
            state,
            screens,
            windows: HashMap::new(),
            events,
            hidden: false,
            ready_sent: false,
        };
        desktop.sync_windows();
        desktop
    }

    /// Une fenêtre par boîte : crée celles qui manquent, oublie celles des boîtes
    /// supprimées.
    fn sync_windows(&mut self) {
        let screens = &self.screens;
        for (i, b) in self.state.boxes.iter().enumerate() {
            self.windows.entry(b.id).or_insert_with(|| {
                let rect = b
                    .placement
                    .resolve(screens)
                    .map(|(_, r)| r)
                    .unwrap_or_else(|| default_rect(screens, i));
                let short: String = b.id.0.simple().to_string().chars().take(8).collect();
                BoxWindow {
                    viewport: ViewportId::from_hash_of(("tidy-boîte", b.id)),
                    title: format!("Tidy · boîte {short}"),
                    spawn: rect,
                    current: rect,
                    placed: false,
                    moved_at: None,
                }
            });
        }
        let ids: Vec<BoxId> = self.state.boxes.iter().map(|b| b.id).collect();
        self.windows.retain(|id, _| ids.contains(id));
    }

    fn save_state(&self) {
        if let Err(e) = self.store.save(&self.state) {
            log::error!("disposition non enregistrée : {e:#}");
        }
    }

    fn create_box(&mut self, at: Option<Point>) {
        let at = at.unwrap_or_else(|| tidy_platform::current().mouse_location());
        let mut b = DeskBox::new(t!("desktop.new_box"));
        let rect = Rect::new(
            at.x - NEW_BOX_SIZE[0] / 2.0,
            at.y - boxwin::TITLE_HEIGHT / 2.0,
            NEW_BOX_SIZE[0],
            NEW_BOX_SIZE[1],
        );
        let screen = screen_at(&self.screens, at);
        let rect = rect.clamped_to(&screen.visible_frame);
        b.placement.remember(&self.screens, &screen, rect);
        self.state.boxes.push(b);
        self.sync_windows();
        self.save_state();
    }

    fn delete_box(&mut self, id: BoxId) {
        // Aucun fichier touché : les éléments de la boîte redeviennent des icônes.
        self.state.remove_box(id);
        self.sync_windows();
        self.save_state();
    }

    /// Événements de l'agent. Rend `true` si l'affichage doit changer.
    fn handle_events(&mut self) -> bool {
        let mut changed = false;
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::DesktopVisibility { hidden } => self.hidden = hidden,
                Event::CreateBox { at } => {
                    self.hidden = false;
                    self.create_box(at);
                }
                Event::SettingsChanged => {
                    self.settings = self.store.load();
                    crate::lang::apply(self.settings.language);
                }
                Event::DesktopChanged => {
                    self.state = self.store.load();
                    self.sync_windows();
                }
                _ => continue,
            }
            changed = true;
        }
        changed
    }

    /// Relit le cadre de la fenêtre ; enregistre la position une fois stable.
    fn track_window(&mut self, id: BoxId, outer: Option<egui::Rect>) {
        let Some(win) = self.windows.get_mut(&id) else {
            return;
        };
        if let Some(r) = outer {
            let rect = Rect::new(r.min.x, r.min.y, r.width(), r.height());
            let moved = (rect.x - win.current.x).abs() > 0.5
                || (rect.y - win.current.y).abs() > 0.5
                || (rect.width - win.current.width).abs() > 0.5
                || (rect.height - win.current.height).abs() > 0.5;
            if moved && win.placed {
                win.current = rect;
                win.moved_at = Some(Instant::now());
            }
        }
        if win.moved_at.is_some_and(|t| t.elapsed() >= SAVE_DELAY) {
            win.moved_at = None;
            let rect = win.current;
            self.screens = tidy_platform::current().screens();
            let center = Point {
                x: rect.x + rect.width / 2.0,
                y: rect.y + rect.height / 2.0,
            };
            let screen = screen_at(&self.screens, center);
            if let Some(b) = self.state.find_box_mut(id) {
                b.placement.remember(&self.screens, &screen, rect);
            }
            self.save_state();
        }
    }
}

/// Écran qui contient `p`, sinon l'écran principal.
fn screen_at(screens: &[Screen], p: Point) -> Screen {
    screens
        .iter()
        .find(|s| s.frame.contains(p))
        .or_else(|| screens.iter().find(|s| s.primary))
        .unwrap_or(&screens[0])
        .clone()
}

/// Place par défaut de la `i`-ième boîte : en haut à droite de l'écran principal, en
/// cascade.
fn default_rect(screens: &[Screen], i: usize) -> Rect {
    let v = screens
        .iter()
        .find(|s| s.primary)
        .unwrap_or(&screens[0])
        .visible_frame;
    let offset = 30.0 * i as f32;
    Rect::new(
        v.right() - NEW_BOX_SIZE[0] - 40.0 - offset,
        v.y + 40.0 + offset,
        NEW_BOX_SIZE[0],
        NEW_BOX_SIZE[1],
    )
    .clamped_to(&v)
}

fn box_builder(win: &BoxWindow) -> ViewportBuilder {
    ViewportBuilder::default()
        .with_title(&win.title)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(true)
        .with_has_shadow(false)
        .with_taskbar(false)
        .with_position([win.spawn.x, win.spawn.y])
        .with_inner_size([win.spawn.width, win.spawn.height])
        .with_min_inner_size(boxwin::MIN_SIZE)
}

impl eframe::App for Desktop {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.handle_events() {
            // Les nouvelles fenêtres n'existent qu'après un passage dans `ui`, même si
            // tout est caché derrière des applis.
            ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::RequestPaintWhileHidden);
            ctx.request_repaint();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut new_box = false;
        let mut delete = None;
        let boxes: Vec<DeskBox> = self.state.boxes.clone();
        for (i, b) in boxes.iter().enumerate() {
            let Some(win) = self.windows.get(&b.id) else {
                continue;
            };
            // Masquer le bureau rend les fenêtres invisibles sans les détruire : elles
            // gardent leur place et leurs réglages système.
            let builder = box_builder(win).with_visible(!self.hidden);
            let viewport = win.viewport;
            let demo = i == 0 && b.name == t!("desktop.demo_box");
            let (out, outer) = ctx.show_viewport_immediate(viewport, builder, |ui, _class| {
                let out = boxwin::draw(ui, b, demo);
                let outer = ui.ctx().input(|i| i.viewport().outer_rect);
                (out, outer)
            });
            new_box |= out.new_box;
            if out.delete {
                delete = Some(b.id);
            }
            if let Some(win) = self.windows.get_mut(&b.id)
                && !win.placed
            {
                win.placed = tidy_platform::current().make_desktop_surface_titled(
                    &win.title,
                    win.current,
                    SurfaceLevel::DesktopIcons,
                );
                ctx.request_repaint();
            }
            self.track_window(b.id, outer);
            if self
                .windows
                .get(&b.id)
                .is_some_and(|w| w.moved_at.is_some())
            {
                ctx.request_repaint_after(SAVE_DELAY);
            }
        }
        if new_box {
            self.create_box(None);
            ctx.request_repaint();
        }
        if let Some(id) = delete {
            self.delete_box(id);
            ctx.request_repaint();
        }
        if !self.ready_sent && !self.hidden && self.windows.values().all(|w| w.placed) {
            self.ready_sent = true;
            let store = self.store.clone();
            std::thread::spawn(move || {
                let _ = ipc::request(&store, &Request::DesktopReady);
            });
        }
    }
}
