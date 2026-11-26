//! Processus du bureau (`tidy --desktop`) : **une fenêtre par boîte** (ADR 011), posée
//! juste au-dessus des icônes du bureau, sous toutes les applis.
//!
//! - Une fenêtre « hôte » de 1×1 point, invisible, porte la boucle d'eframe ; chaque
//!   boîte est une fenêtre secondaire (viewport immédiat) à sa taille. Menus,
//!   confirmations, bandeau des non affectées et bouton « Créer une boîte » sont de
//!   petites fenêtres ouvertes le temps qu'il faut ([`overlay`]).
//! - Les événements de l'agent sont traités dans [`eframe::App::logic`], qui tourne
//!   même quand toutes les fenêtres sont cachées derrière des applis ; un changement
//!   de structure (boîte créée, bureau montré) demande une image malgré tout
//!   (`RequestPaintWhileHidden`). Au repos, rien ne se redessine.
//! - Le système déplace et redimensionne les fenêtres ; la position finale est aimantée
//!   (sauf Maj), puis enregistrée par configuration d'écrans.
//! - **Seul écrivain de `desktop.json`** : les commandes des autres processus arrivent
//!   par `Event::Desktop`. Ranger un élément ne touche jamais au fichier ; seuls
//!   « Renommer » et « Mettre à la corbeille » (avec confirmation) agissent sur le
//!   disque, à la demande explicite de l'utilisateur.
//!
//! Le processus suit l'agent : s'il disparaît (fin de l'abonnement aux événements), le
//! bureau rend les icônes du système s'il le faut, puis s'arrête.

mod action;
mod boxwin;
mod hotkeys;
mod items;
mod menu;
mod ops;
mod overlay;
mod portal;
mod roll;
mod textures;
mod widgets;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use egui::{ViewportBuilder, ViewportCommand, ViewportId, pos2};
use tidy_core::events::Event;
use tidy_core::geometry::{Point, Rect};
use tidy_core::model::{Appearance, BoxId, BoxKind, DeskBox, SortKey, Tab};
use tidy_core::rules::ItemInfo;
use tidy_core::scan::scan_desktop_located;
use tidy_core::screen::Screen;
use tidy_core::snap::{Edges, SnapContext, snap, snap_resize};
use tidy_core::store::{desktop_dir, public_desktop_dir};
use tidy_core::{DesktopState, Settings, Store};
use tidy_platform::SurfaceLevel;
use tidy_platform::finder::{self, SystemFinder};
use tidy_ui::t;

use crate::ipc::{self, Request, Response};
use action::Action;
use boxwin::{BoxUi, BoxView, NameEdit, PortalView, TITLE_HEIGHT};
use items::{GridGeom, ItemDir, RenameEdit, Section};
use overlay::{Overlay, PopupKind};
use portal::{DirWatcher, Portal, PortalRefusal};

/// Taille d'une nouvelle boîte, en points.
const NEW_BOX_SIZE: [f32; 2] = [320.0, 220.0];
/// Délai après le dernier mouvement d'une fenêtre avant d'aimanter et d'enregistrer.
const SAVE_DELAY: Duration = Duration::from_millis(400);
/// Durée pendant laquelle le raccourci d'une boîte la garde devant les applis.
const HOTKEY_RAISE: Duration = Duration::from_secs(4);
/// Au-delà, on cesse d'attendre que la fenêtre prenne le cadre demandé.
const EXPECT_TIMEOUT: Duration = Duration::from_secs(1);

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
    let with_agent = match ipc::subscribe(
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
        Ok(()) => true,
        Err(e) => {
            // Lancé à la main, sans agent : on continue (essais).
            log::warn!("pas d'agent ({e:#}) : bureau autonome");
            false
        }
    };

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
            let platform = tidy_platform::current();
            platform.make_desktop_surface(cc, host, SurfaceLevel::DesktopIcons);
            tidy_ui::theme::apply(&cc.egui_ctx);
            // Pas de curseur clignotant : il redessinerait toutes les boîtes deux fois par
            // seconde pendant un renommage.
            cc.egui_ctx
                .style_mut_of(egui::Theme::Dark, |s| s.visuals.text_cursor.blink = false);
            let ctx = cc.egui_ctx.clone();
            let _ = ctx_cell.set(ctx.clone());
            let channels = Channels::install(&ctx, &settings);
            Ok(Box::new(Desktop::new(
                store, settings, screens, events, ctx, channels, with_agent,
            )))
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

/// Canaux des fils et rappels système vers la boucle.
struct Channels {
    /// Rectangles tracés sur le bureau (« Créer une boîte en dessinant »).
    drawn: Receiver<Rect>,
    /// Raccourcis des boîtes.
    hotkey: Receiver<BoxId>,
    /// Les écrans ont changé (branchement, résolution, échelle).
    screens: Receiver<()>,
    hotkeys: hotkeys::BoxHotkeys,
    /// Résultats de « Appliquer les règles ».
    replies: Receiver<String>,
    reply_tx: Sender<String>,
}

impl Channels {
    /// Installe les rappels système. Sur le thread principal, boucle lancée.
    fn install(ctx: &egui::Context, settings: &Settings) -> Self {
        let (draw_tx, drawn) = mpsc::channel();
        if settings.quick_actions.draw_to_create {
            let draw_tx = std::sync::Mutex::new(draw_tx);
            let wake = ctx.clone();
            let watching = tidy_platform::current().watch_desktop_drag(Box::new(move |rect| {
                if let Ok(tx) = draw_tx.lock() {
                    let _ = tx.send(rect);
                }
                wake.request_repaint();
            }));
            log::info!("création de boîte en dessinant : {watching}");
        }
        let (screens_tx, screens) = mpsc::channel();
        let screens_tx = std::sync::Mutex::new(screens_tx);
        let wake = ctx.clone();
        let watching = tidy_platform::current().watch_screens(Box::new(move || {
            if let Ok(tx) = screens_tx.lock() {
                let _ = tx.send(());
            }
            wake.request_repaint();
        }));
        log::info!("surveillance des écrans : {watching}");
        let (hot_tx, hotkey) = mpsc::channel();
        let wake = ctx.clone();
        let hotkeys = hotkeys::BoxHotkeys::new(hot_tx, move || wake.request_repaint());
        let (reply_tx, replies) = mpsc::channel();
        Self {
            drawn,
            hotkey,
            screens,
            hotkeys,
            replies,
            reply_tx,
        }
    }
}

/// Masque les icônes rangées (Windows) sur un fil à part, une demande à la fois : les
/// demandes rapprochées n'en font qu'une.
struct IconSync {
    tx: Sender<()>,
}

impl IconSync {
    fn new(store: Store) -> Self {
        let (tx, rx) = mpsc::channel::<()>();
        let spawned = std::thread::Builder::new()
            .name("tidy-icônes-rangées".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    // Regrouper les demandes arrivées entre-temps.
                    std::thread::sleep(Duration::from_millis(150));
                    while rx.try_recv().is_ok() {}
                    crate::icons::log_report(crate::icons::sync(&store, &[]), true);
                }
            });
        if let Err(e) = spawned {
            log::warn!("fil des icônes rangées : {e}");
        }
        Self { tx }
    }

    fn request(&self) {
        let _ = self.tx.send(());
    }
}

/// État d'une fenêtre de boîte.
struct BoxWindow {
    viewport: ViewportId,
    title: String,
    /// Cadre de création (constant, pour qu'egui ne replace pas la fenêtre).
    spawn: Rect,
    /// Cadre complet (déroulé), relu sur la fenêtre.
    current: Rect,
    /// Fenêtre native réglée (niveau, bureau…).
    placed: bool,
    /// Déplacée ou redimensionnée depuis : à aimanter puis enregistrer.
    moved_at: Option<Instant>,
    /// Cadre au début du mouvement en cours (déplacement ou redimensionnement).
    move_origin: Option<Rect>,
    /// Maj enfoncée pendant le dernier mouvement (pas d'aimantation).
    shift: bool,
    /// Cadre demandé à la fenêtre : on ne relit pas sa position avant qu'elle l'ait pris.
    expect: Option<(Rect, Instant)>,
    /// Affichée enroulée.
    rolled: bool,
    /// Enroulée vers le bas (collée au bas de l'écran).
    anchored_bottom: bool,
    hovered: bool,
    last_hover: Instant,
    /// Passée devant les applis (accès rapide, raccourci).
    raised: bool,
    ui: BoxUi,
    sections: Vec<Section>,
    sections_key: (u64, u64),
    /// Grille libre affichée, en coordonnées globales (case visée par un dépôt).
    grid: Option<GridGeom>,
}

impl BoxWindow {
    /// Cadre affiché (enroulé ou non).
    fn shown(&self, screens: &[Screen]) -> Rect {
        if self.rolled {
            if self.anchored_bottom {
                Rect::new(
                    self.current.x,
                    self.current.bottom() - TITLE_HEIGHT,
                    self.current.width,
                    TITLE_HEIGHT,
                )
            } else {
                Rect::new(
                    self.current.x,
                    self.current.y,
                    self.current.width,
                    TITLE_HEIGHT,
                )
            }
        } else {
            let _ = screens;
            self.current
        }
    }
}

/// Glisser interne (repli quand le glisser du système n'est pas disponible).
struct InternalDrag {
    from: BoxId,
    names: Vec<String>,
    /// Éléments d'un portail (fichiers hors du bureau).
    portal: bool,
}

struct Desktop {
    store: Store,
    settings: Settings,
    state: DesktopState,
    screens: Vec<Screen>,
    windows: HashMap<BoxId, BoxWindow>,
    events: Receiver<Event>,
    channels: Channels,
    ctx: egui::Context,
    hidden: bool,
    ready_sent: bool,
    /// Dossier du bureau et ses éléments.
    desktop_dir: Option<PathBuf>,
    /// Bureau public (Windows) et noms des éléments qui en viennent.
    public_dir: Option<PathBuf>,
    public_names: Arc<HashSet<String>>,
    files: HashMap<String, ItemInfo>,
    /// Surveillance du bureau quand aucun agent ne la fait (bureau autonome).
    desktop_watch: Option<DirWatcher>,
    /// Version de l'état et des fichiers (caches de l'affichage).
    generation: u64,
    icons: textures::IconCache,
    portals: HashMap<BoxId, Portal>,
    overlay: Overlay,
    drag: Option<InternalDrag>,
    /// Boîtes passées devant par leur raccourci, jusqu'à quand.
    raised_until: HashMap<BoxId, Instant>,
    quick_access: bool,
    icon_sync: IconSync,
    /// Fenêtres des widgets (lots H et I).
    widgets: widgets::WidgetHost,
}

impl Desktop {
    #[allow(clippy::too_many_arguments)]
    fn new(
        store: Store,
        settings: Settings,
        screens: Vec<Screen>,
        events: Receiver<Event>,
        ctx: egui::Context,
        channels: Channels,
        with_agent: bool,
    ) -> Self {
        // Pas de boîte de démonstration : les premières boîtes viennent de l'assistant
        // (démarrage rapide) ou de l'utilisateur.
        let state: DesktopState = store.load();
        let desktop_dir = desktop_dir();
        let desktop_watch = if with_agent {
            None
        } else {
            desktop_dir
                .as_deref()
                .and_then(|d| DirWatcher::new(d, &ctx))
        };
        let icon_sync = IconSync::new(store.clone());
        let widget_host = widgets::WidgetHost::new(&store);
        let mut desktop = Self {
            icons: textures::IconCache::new(ctx.clone()),
            store,
            settings,
            state,
            screens,
            windows: HashMap::new(),
            events,
            channels,
            ctx,
            hidden: false,
            ready_sent: false,
            desktop_dir,
            public_dir: public_desktop_dir(),
            public_names: Arc::default(),
            files: HashMap::new(),
            desktop_watch,
            generation: 0,
            portals: HashMap::new(),
            overlay: Overlay::default(),
            drag: None,
            raised_until: HashMap::new(),
            quick_access: false,
            icon_sync,
            widgets: widget_host,
        };
        desktop.rescan_desktop();
        desktop.sync_windows();
        desktop
    }

    fn look(&self, b: &DeskBox) -> Appearance {
        b.appearance
            .clone()
            .unwrap_or_else(|| self.settings.default_appearance.clone())
    }

    /// Relit le dossier du bureau. `false` si la lecture a échoué.
    fn rescan_desktop(&mut self) -> bool {
        self.generation += 1;
        let Some(dir) = &self.desktop_dir else {
            return false;
        };
        let dirs: Vec<PathBuf> = std::iter::once(dir.clone())
            .chain(self.public_dir.clone())
            .collect();
        match scan_desktop_located(&dirs) {
            Ok(items) => {
                self.public_names = Arc::new(
                    items
                        .iter()
                        .filter(|(_, i)| *i > 0)
                        .map(|(e, _)| e.info.name.clone())
                        .collect(),
                );
                self.files = items
                    .into_iter()
                    .map(|(e, _)| (e.info.name.clone(), e.info))
                    .collect();
                true
            }
            Err(e) => {
                log::warn!("lecture du bureau {} : {e}", dir.display());
                false
            }
        }
    }

    /// Les fichiers du bureau ont changé : relire, puis oublier les icônes hors boîte
    /// dont le fichier n'existe plus (aucune commande ne le fait : c'est au bureau).
    fn desktop_files_changed(&mut self) {
        if !self.rescan_desktop() {
            return;
        }
        if ops::prune_loose(&mut self.state, |name| self.files.contains_key(name)) {
            self.save(false);
        }
    }

    /// Une fenêtre par boîte : crée celles qui manquent, oublie celles des boîtes
    /// supprimées. Même chose pour les portails.
    fn sync_windows(&mut self) {
        let screens = &self.screens;
        for (i, b) in self.state.boxes.iter().enumerate() {
            self.windows.entry(b.id).or_insert_with(|| {
                let (screen, rect) = match b.placement.resolve(screens) {
                    Some((id, r)) => (screens.iter().find(|s| s.id == id).cloned(), r),
                    None => (None, default_rect(screens, i)),
                };
                let bounds = screen
                    .unwrap_or_else(|| screen_at(screens, center(rect)))
                    .visible_frame;
                let (spawn, anchored_bottom) = if b.rolled_up {
                    roll::rolled(rect, TITLE_HEIGHT, bounds)
                } else {
                    (rect, false)
                };
                let short: String = b.id.0.simple().to_string().chars().take(8).collect();
                BoxWindow {
                    viewport: ViewportId::from_hash_of(("tidy-boîte", b.id)),
                    title: format!("Tidy · boîte {short}"),
                    spawn,
                    current: rect,
                    placed: false,
                    moved_at: None,
                    move_origin: None,
                    shift: false,
                    expect: None,
                    rolled: b.rolled_up,
                    anchored_bottom,
                    hovered: false,
                    last_hover: Instant::now(),
                    raised: false,
                    ui: BoxUi::default(),
                    sections: Vec::new(),
                    sections_key: (u64::MAX, u64::MAX),
                    grid: None,
                }
            });
            if let BoxKind::Portal { path } = &b.kind {
                let ctx = &self.ctx;
                let portal = self
                    .portals
                    .entry(b.id)
                    .or_insert_with(|| Portal::new(path.clone(), ctx));
                if portal.root() != path {
                    *portal = Portal::new(path.clone(), ctx);
                }
            }
        }
        let ids: Vec<BoxId> = self.state.boxes.iter().map(|b| b.id).collect();
        self.windows.retain(|id, _| ids.contains(id));
        self.portals.retain(|id, _| ids.contains(id));
        self.channels.hotkeys.sync(&self.state.boxes);
        self.generation += 1;
    }

    /// Enregistre l'état. `content` : le contenu des boîtes a changé (icônes rangées à
    /// masquer ou à rendre).
    fn save(&mut self, content: bool) {
        self.generation += 1;
        for tab in self.state.boxes.iter_mut().flat_map(|b| b.tabs.iter_mut()) {
            tab.prune_cells();
        }
        if let Err(e) = self.store.save(&self.state) {
            log::error!("disposition non enregistrée : {e:#}");
        }
        if content {
            self.icon_sync.request();
        }
    }

    /// Ajoute une boîte sur `rect` (coordonnées globales).
    fn add_box(&mut self, mut b: DeskBox, rect: Rect) -> BoxId {
        let screen = screen_at(&self.screens, center(rect));
        let rect = rect.clamped_to(&screen.visible_frame);
        b.placement.remember(&self.screens, &screen, rect);
        let id = b.id;
        self.state.boxes.push(b);
        self.sync_windows();
        self.save(false);
        self.ctx
            .send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::RequestPaintWhileHidden);
        id
    }

    /// Nouvelle boîte sous la souris (ou à `at`).
    fn create_box(&mut self, at: Option<Point>) {
        let at = at.unwrap_or_else(|| tidy_platform::current().mouse_location());
        let rect = Rect::new(
            at.x - NEW_BOX_SIZE[0] / 2.0,
            at.y - TITLE_HEIGHT / 2.0,
            NEW_BOX_SIZE[0],
            NEW_BOX_SIZE[1],
        );
        self.add_box(DeskBox::new(t!("desktop.new_box")), rect);
    }

    /// Cadre d'une nouvelle boîte à côté de `near` : à droite, sinon à gauche, sinon
    /// dessous.
    fn rect_near(&self, near: BoxId) -> Rect {
        let size = NEW_BOX_SIZE;
        let Some(win) = self.windows.get(&near) else {
            return default_rect(&self.screens, self.state.boxes.len());
        };
        let r = win.shown(&self.screens);
        let bounds = screen_at(&self.screens, center(r)).visible_frame;
        let gap = self.settings.layout.spacing.max(4.0);
        let candidates = [
            Rect::new(r.right() + gap, r.y, size[0], size[1]),
            Rect::new(r.x - gap - size[0], r.y, size[0], size[1]),
            Rect::new(r.x, r.bottom() + gap, size[0], size[1]),
        ];
        candidates
            .into_iter()
            .find(|c| {
                c.x >= bounds.x
                    && c.right() <= bounds.right()
                    && c.y >= bounds.y
                    && c.bottom() <= bounds.bottom()
            })
            .unwrap_or(candidates[0])
    }

    fn delete_box(&mut self, id: BoxId) {
        // Aucun fichier touché : les éléments de la boîte redeviennent des icônes.
        self.state.remove_box(id);
        self.sync_windows();
        self.save(true);
    }

    /// Dossier des éléments d'une boîte : le bureau, ou le dossier affiché du portail.
    fn dir_of(&self, b: BoxId) -> Option<ItemDir> {
        match self.portals.get(&b) {
            Some(p) => Some(ItemDir::plain(p.current())),
            None => Some(ItemDir {
                main: self.desktop_dir.clone()?,
                public: self
                    .public_dir
                    .clone()
                    .map(|d| (d, self.public_names.clone())),
            }),
        }
    }

    fn is_portal(&self, b: BoxId) -> bool {
        self.portals.contains_key(&b)
    }

    fn notice(&mut self, b: BoxId, text: impl Into<String>) {
        if let Some(win) = self.windows.get_mut(&b) {
            win.ui.notice(text);
            self.ctx.request_repaint();
        }
    }

    /// Événements de l'agent et des fils. Rend `true` si l'affichage doit changer.
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
                    self.widgets.settings_changed(&self.settings);
                }
                Event::DesktopChanged => {
                    self.state = self.store.load();
                    self.sync_windows();
                    self.icon_sync.request();
                }
                Event::Desktop { command } => {
                    // Seul écrivain de `desktop.json` (voir `DesktopCommand`).
                    self.state.apply(command);
                    self.sync_windows();
                    self.save(true);
                }
                Event::DesktopFilesChanged => self.desktop_files_changed(),
                Event::ScreensChanged { screens } => self.screens_changed(screens),
                Event::QuickAccess { active } => {
                    self.quick_access = active;
                    let ids: Vec<BoxId> = self
                        .state
                        .boxes
                        .iter()
                        .filter(|b| !b.exclude_from_quick_access)
                        .map(|b| b.id)
                        .collect();
                    for id in ids {
                        let keep = self.raised_until.contains_key(&id);
                        self.set_raised(id, active || keep);
                    }
                }
                _ => continue,
            }
            changed = true;
        }
        // Écrans changés : chaque boîte reprend sa place pour la nouvelle configuration.
        let mut screens_changed = false;
        while self.channels.screens.try_recv().is_ok() {
            screens_changed = true;
        }
        if screens_changed {
            self.screens_changed(tidy_platform::current().screens());
            changed = true;
        }
        // Rectangle tracé sur le bureau.
        while let Ok(rect) = self.channels.drawn.try_recv() {
            if rect.width >= 24.0 && rect.height >= 24.0 {
                self.overlay.offer_draw(rect);
                changed = true;
            }
        }
        // Raccourci d'une boîte : devant quelques secondes.
        while let Ok(id) = self.channels.hotkey.try_recv() {
            self.raised_until.insert(id, Instant::now() + HOTKEY_RAISE);
            self.set_raised(id, true);
            if let Some(win) = self.windows.get(&id) {
                self.ctx
                    .send_viewport_cmd_to(win.viewport, ViewportCommand::Focus);
            }
            changed = true;
        }
        let now = Instant::now();
        let expired: Vec<BoxId> = self
            .raised_until
            .iter()
            .filter(|(_, until)| **until <= now)
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            self.raised_until.remove(&id);
            if !self.quick_access {
                self.set_raised(id, false);
            }
            changed = true;
        }
        if let Some(next) = self.raised_until.values().min() {
            self.ctx
                .request_repaint_after(next.saturating_duration_since(now));
        }
        // Réponses de l'agent (« Appliquer les règles »).
        while let Ok(text) = self.channels.replies.try_recv() {
            self.overlay.pill_message(text);
            changed = true;
        }
        // Dossiers surveillés.
        if self
            .desktop_watch
            .as_ref()
            .is_some_and(DirWatcher::take_dirty)
        {
            self.desktop_files_changed();
            changed = true;
        }
        let ctx = self.ctx.clone();
        for portal in self.portals.values_mut() {
            changed |= portal.refresh(&ctx);
        }
        changed
    }

    /// Les écrans ont changé : chaque boîte reprend la place mémorisée pour la nouvelle
    /// configuration.
    fn screens_changed(&mut self, screens: Vec<Screen>) {
        if screens.is_empty() {
            return;
        }
        self.screens = screens;
        for (i, b) in self.state.boxes.iter().enumerate() {
            let Some(win) = self.windows.get_mut(&b.id) else {
                continue;
            };
            let rect = b
                .placement
                .resolve(&self.screens)
                .map(|(_, r)| r)
                .unwrap_or_else(|| default_rect(&self.screens, i));
            win.current = rect;
            let shown = win.shown(&self.screens);
            move_window(&self.ctx, win, shown);
        }
    }

    /// Passe une boîte devant les applis, ou la remet au niveau du bureau.
    fn set_raised(&mut self, id: BoxId, raised: bool) {
        let Some(win) = self.windows.get_mut(&id) else {
            return;
        };
        if win.raised == raised {
            return;
        }
        win.raised = raised;
        // Sans effet si la plateforme ne sait pas le faire : la boîte reste au niveau
        // du bureau.
        tidy_platform::current().raise_desktop_surface(&win.title, raised);
        self.ctx
            .send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::RequestPaintWhileHidden);
        self.ctx.request_repaint();
    }

    /// Recalcule le contenu affiché d'une boîte si l'état ou les fichiers ont changé.
    fn refresh_sections(&mut self, id: BoxId) {
        let portal_gen = self.portals.get(&id).map_or(0, |p| p.generation);
        let key = (self.generation, portal_gen);
        let Some(win) = self.windows.get(&id) else {
            return;
        };
        if win.sections_key == key {
            return;
        }
        let Some(b) = self.state.find_box(id) else {
            return;
        };
        let entries = match self.portals.get(&id) {
            Some(p) => items::dir_entries(p.items.clone(), b.sort),
            None => b
                .tabs
                .get(b.active_tab.min(b.tabs.len().saturating_sub(1)))
                .map(|tab| items::tab_entries(tab, &self.files, b.sort))
                .unwrap_or_default(),
        };
        let sections = items::sections(
            entries,
            b.sort.group_by_date,
            jiff::Timestamp::now(),
            &jiff::tz::TimeZone::system(),
        );
        if let Some(win) = self.windows.get_mut(&id) {
            win.sections = sections;
            win.sections_key = key;
        }
    }

    /// Boîte affichée sous `p` (coordonnées globales).
    fn box_under(&self, p: Point) -> Option<BoxId> {
        self.state.boxes.iter().rev().find_map(|b| {
            let win = self.windows.get(&b.id)?;
            win.shown(&self.screens).contains(p).then_some(b.id)
        })
    }

    /// Menu ☰ d'une boîte.
    fn box_menu(
        state: &DesktopState,
        settings: &Settings,
        b: BoxId,
        page: menu::Page,
    ) -> Vec<menu::Entry> {
        let Some(deskbox) = state.find_box(b) else {
            return Vec::new();
        };
        let others: Vec<(BoxId, String)> = state
            .boxes
            .iter()
            .filter(|o| o.id != b && matches!(o.kind, BoxKind::Normal))
            .map(|o| (o.id, o.name.clone()))
            .collect();
        let icon_size = deskbox
            .appearance
            .as_ref()
            .unwrap_or(&settings.default_appearance)
            .icon_size;
        menu::box_menu(deskbox, page, &others, icon_size)
    }

    /// Relit le cadre de la fenêtre ; une fois stable, l'aimante puis l'enregistre.
    fn track_window(&mut self, id: BoxId, outer: Option<egui::Rect>, shift: bool) {
        let Some(win) = self.windows.get_mut(&id) else {
            return;
        };
        if let Some(r) = outer {
            let rect = Rect::new(r.min.x, r.min.y, r.width(), r.height());
            if let Some((expected, since)) = win.expect
                && (near(rect, expected) || since.elapsed() > EXPECT_TIMEOUT)
            {
                win.expect = None;
            }
            if win.expect.is_none() {
                let full = if win.rolled {
                    roll::unrolled(rect, win.current.height, win.anchored_bottom)
                } else {
                    rect
                };
                if !near(full, win.current) && win.placed {
                    if win.moved_at.is_none() {
                        win.move_origin = Some(win.current);
                    }
                    win.current = full;
                    win.moved_at = Some(Instant::now());
                    win.shift = shift;
                }
            }
        }
        if !win.moved_at.is_some_and(|t| t.elapsed() >= SAVE_DELAY) {
            return;
        }
        win.moved_at = None;
        let origin = win.move_origin.take();
        self.screens = tidy_platform::current().screens();
        if self.screens.is_empty() {
            return;
        }
        let Some(win) = self.windows.get(&id) else {
            return;
        };
        let shown = win.shown(&self.screens);
        let screen = screen_at(&self.screens, center(shown));
        // Aimantation (sauf Maj) : déplacement, ou redimensionnement par les bords tirés.
        if !win.shift {
            let others: Vec<Rect> = self
                .windows
                .iter()
                .filter(|(other, _)| **other != id)
                .map(|(_, w)| w.shown(&self.screens))
                .filter(|r| screen.frame.contains(center(*r)))
                .collect();
            let look = self
                .state
                .find_box(id)
                .map(|b| self.look(b))
                .unwrap_or_default();
            let cell = items::grid_metrics(1000.0, look.icon_size).cell.y;
            let ctx = SnapContext {
                others: &others,
                bounds: screen.visible_frame,
                cell,
                settings: &self.settings.layout,
            };
            let full = win.current;
            let mut snapped = match origin {
                Some(o) if !win.rolled && !same_size(o, full) => {
                    snap_resize(shown, resized_edges(o, full), &ctx)
                }
                _ => snap(shown, &ctx),
            };
            if let Some(win) = self.windows.get_mut(&id) {
                if win.rolled {
                    snapped.height = TITLE_HEIGHT;
                    win.current = roll::unrolled(snapped, win.current.height, win.anchored_bottom);
                } else {
                    win.current = snapped;
                }
                if !near(snapped, shown) {
                    move_window(&self.ctx, win, snapped);
                }
            }
        }
        let Some(win) = self.windows.get(&id) else {
            return;
        };
        let full = win.current;
        let screen = screen_at(&self.screens, center(full));
        if let Some(b) = self.state.find_box_mut(id) {
            b.placement.remember(&self.screens, &screen, full);
        }
        self.save(false);
    }

    /// Enroule ou déroule la fenêtre si l'état voulu a changé.
    fn apply_roll(&mut self, id: BoxId) {
        let Some(b) = self.state.find_box(id) else {
            return;
        };
        let busy = self.overlay.busy_with(id) || self.drag.as_ref().is_some_and(|d| d.from == id);
        let Some(win) = self.windows.get(&id) else {
            return;
        };
        let bounds = screen_at(&self.screens, center(win.current)).visible_frame;
        let auto = roll::auto_roll(
            b.auto_roll,
            self.settings.layout.roll_up_at_edge,
            win.current,
            bounds,
        );
        let away = !win.hovered && win.last_hover.elapsed() >= roll::AUTO_ROLL_DELAY;
        let want = b.rolled_up || (auto && away && !busy && !win.ui.editing());
        if auto && !win.hovered && !away && !b.rolled_up {
            self.ctx.request_repaint_after(
                roll::AUTO_ROLL_DELAY.saturating_sub(win.last_hover.elapsed()),
            );
        }
        if want == win.rolled {
            return;
        }
        let Some(win) = self.windows.get_mut(&id) else {
            return;
        };
        let target = if want {
            let (r, bottom) = roll::rolled(win.current, TITLE_HEIGHT, bounds);
            win.anchored_bottom = bottom;
            r
        } else {
            win.current
        };
        win.rolled = want;
        move_window(&self.ctx, win, target);
    }

    /// Applique une demande de l'utilisateur.
    fn apply(&mut self, action: Action) {
        let platform = tidy_platform::current();
        match action {
            Action::Open { b, names } => {
                let Some(dir) = self.dir_of(b) else { return };
                for name in names {
                    if let Err(e) = platform.open(&dir.join(&name)) {
                        log::warn!("ouverture de {name} : {e:#}");
                        self.notice(b, t!("box.open_failed", name = name));
                    }
                }
            }
            Action::ItemMenu { b, names } => {
                let Some(dir) = self.dir_of(b) else { return };
                let paths: Vec<PathBuf> = names.iter().map(|n| dir.join(n)).collect();
                let title = self
                    .windows
                    .get(&b)
                    .map(|w| w.title.clone())
                    .unwrap_or_default();
                match platform.show_file_menu(&title, &paths) {
                    Ok(true) => {}
                    Ok(false) | Err(_) => {
                        let entries = menu::item_menu(b, &names, self.is_portal(b));
                        self.overlay.open(PopupKind::Menu { entries }, mouse_pos());
                    }
                }
            }
            Action::Reveal { b, name } => {
                if let Some(dir) = self.dir_of(b)
                    && let Err(e) = platform.reveal(&dir.join(&name))
                {
                    log::warn!("afficher {name} : {e:#}");
                }
            }
            Action::StartRename { b, name } => {
                if let Some(win) = self.windows.get_mut(&b) {
                    win.ui.items.renaming = Some(RenameEdit {
                        old: name.clone(),
                        text: name,
                        focus: true,
                    });
                    self.ctx
                        .send_viewport_cmd_to(win.viewport, ViewportCommand::Focus);
                }
            }
            Action::Rename { b, old, new } => self.rename(b, &old, &new),
            Action::AskTrash { b, names } => {
                let body = names
                    .iter()
                    .take(3)
                    .map(|n| tidy_ui::widgets::truncate(n, 40))
                    .collect::<Vec<_>>()
                    .join("\n");
                let body = if names.len() > 3 {
                    format!("{body}\n…")
                } else {
                    body
                };
                let title = if names.len() == 1 {
                    t!("box.trash_title_one").to_owned()
                } else {
                    t!("box.trash_title", n = names.len())
                };
                let anchor = self.popup_anchor(b);
                self.overlay.open(
                    PopupKind::Confirm {
                        title,
                        body,
                        confirm: t!("box.trash_confirm").into(),
                        danger: true,
                        action: Action::Trash { b, names },
                    },
                    anchor,
                );
            }
            Action::Trash { b, names } => {
                let Some(dir) = self.dir_of(b) else { return };
                let paths: Vec<PathBuf> = names.iter().map(|n| dir.join(n)).collect();
                match platform.trash(&paths) {
                    Ok(()) => {
                        if !self.is_portal(b) {
                            ops::forget_items(&mut self.state, &names);
                            self.save(true);
                        }
                        self.rescan_dir_of(b);
                    }
                    Err(e) => {
                        log::warn!("corbeille : {e:#}");
                        self.notice(b, t!("box.trash_failed"));
                    }
                }
            }
            Action::CopyPath { b, names } => {
                let Some(dir) = self.dir_of(b) else { return };
                let text = names
                    .iter()
                    .map(|n| dir.join(n).display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                self.ctx.copy_text(text);
                self.notice(b, t!("box.path_copied"));
            }
            Action::RemoveFromBox { b: _, names } => {
                ops::remove_from_box(&mut self.state, &names);
                self.save(true);
            }
            Action::DropFiles { b, paths, before } => self.drop_files(b, &paths, before),
            Action::StartDrag { b, names } => {
                let Some(dir) = self.dir_of(b) else { return };
                let paths: Vec<PathBuf> = names.iter().map(|n| dir.join(n)).collect();
                let title = self
                    .windows
                    .get(&b)
                    .map(|w| w.title.clone())
                    .unwrap_or_default();
                match platform.start_file_drag(&title, &paths) {
                    Ok(true) => {}
                    _ => {
                        self.drag = Some(InternalDrag {
                            from: b,
                            names,
                            portal: self.is_portal(b),
                        });
                    }
                }
            }
            Action::EndDrag { b, before } => self.end_drag(b, before),
            Action::BoxMenu { b } => {
                self.overlay.open(
                    PopupKind::BoxMenu {
                        b,
                        page: menu::Page::Main,
                    },
                    mouse_pos(),
                );
            }
            Action::StartRenameBox { b } => {
                let name = self
                    .state
                    .find_box(b)
                    .map(|x| x.name.clone())
                    .unwrap_or_default();
                if let Some(win) = self.windows.get_mut(&b) {
                    win.ui.renaming_title = Some(NameEdit {
                        text: name,
                        focus: true,
                    });
                    self.ctx
                        .send_viewport_cmd_to(win.viewport, ViewportCommand::Focus);
                }
            }
            Action::RenameBox { b, name } => {
                if !name.is_empty()
                    && let Some(x) = self.state.find_box_mut(b)
                {
                    x.name = name;
                    self.save(false);
                }
            }
            Action::SetSort { b, key } => self.edit_box(b, |x| x.sort.key = key),
            Action::ToggleDescending { b } => {
                self.edit_box(b, |x| x.sort.descending = !x.sort.descending);
            }
            Action::ToggleGroupByDate { b } => {
                self.edit_box(b, |x| x.sort.group_by_date = !x.sort.group_by_date);
            }
            Action::SetView { b, view } => self.edit_box(b, |x| x.view = view),
            Action::CompactGrid { b } => self.edit_box(b, |x| {
                let tab = x.active_tab.min(x.tabs.len().saturating_sub(1));
                if let Some(t) = x.tabs.get_mut(tab) {
                    t.cells.clear();
                }
            }),
            Action::SetIconSize { b, size } => {
                let default = self.settings.default_appearance.clone();
                self.edit_box(b, |x| {
                    x.appearance.get_or_insert(default).icon_size = size;
                });
            }
            Action::ToggleRoll { b } => self.edit_box(b, |x| x.rolled_up = !x.rolled_up),
            Action::ToggleAutoRoll { b } => self.edit_box(b, |x| x.auto_roll = !x.auto_roll),
            Action::Appearance { .. } => open_manager("boxes"),
            Action::NewPortal { near } => self.new_portal(near),
            Action::NewBox { near, rect } => match (rect, near) {
                (Some(r), _) => {
                    let r = Rect::new(
                        r.x,
                        r.y,
                        r.width.max(boxwin::MIN_SIZE[0]),
                        r.height.max(boxwin::MIN_SIZE[1]),
                    );
                    self.add_box(DeskBox::new(t!("desktop.new_box")), r);
                }
                (None, Some(near)) => {
                    let r = self.rect_near(near);
                    self.add_box(DeskBox::new(t!("desktop.new_box")), r);
                }
                (None, None) => self.create_box(None),
            },
            Action::NewTab { b } => {
                self.edit_box(b, |x| {
                    if x.tabs.len() == 1 && x.tabs[0].name.is_empty() {
                        x.tabs[0].name = x.name.clone();
                    }
                    x.tabs
                        .push(Tab::new(t!("box.tab_untitled", n = x.tabs.len() + 1)));
                    x.active_tab = x.tabs.len() - 1;
                });
            }
            Action::SelectTab { b, index } => {
                self.edit_box(b, |x| {
                    x.active_tab = index.min(x.tabs.len().saturating_sub(1))
                });
                if let Some(win) = self.windows.get_mut(&b) {
                    win.ui.items.selected.clear();
                }
            }
            Action::RenameTab { b, index, name } => self.edit_box(b, |x| {
                if let Some(tab) = x.tabs.get_mut(index) {
                    tab.name = name;
                }
            }),
            Action::MergeWith { b, other } => {
                if ops::merge_boxes(&mut self.state, b, other) {
                    self.sync_windows();
                    self.save(true);
                }
            }
            Action::DetachTab { b, index } => {
                let rect = self.rect_near(b);
                if let Some(new) = ops::detach_tab(&mut self.state, b, index) {
                    let screen = screen_at(&self.screens, center(rect));
                    let rect = rect.clamped_to(&screen.visible_frame);
                    if let Some(x) = self.state.find_box_mut(new) {
                        x.placement.remember(&self.screens, &screen, rect);
                    }
                    self.sync_windows();
                    self.save(true);
                    self.ctx.send_viewport_cmd_to(
                        ViewportId::ROOT,
                        ViewportCommand::RequestPaintWhileHidden,
                    );
                }
            }
            Action::AskDeleteBox { b } => {
                let Some(x) = self.state.find_box(b) else {
                    return;
                };
                if x.items().next().is_none() {
                    self.delete_box(b);
                    return;
                }
                let title = t!("box.delete_title", name = x.name.clone());
                let anchor = self.popup_anchor(b);
                self.overlay.open(
                    PopupKind::Confirm {
                        title,
                        body: t!("box.delete_body").into(),
                        confirm: t!("box.delete_confirm").into(),
                        danger: true,
                        action: Action::DeleteBox { b },
                    },
                    anchor,
                );
            }
            Action::DeleteBox { b } => self.delete_box(b),
            Action::PortalEnter { b, name } => {
                let ctx = self.ctx.clone();
                if let Some(p) = self.portals.get_mut(&b) {
                    p.enter(&name, &ctx);
                }
                if let Some(win) = self.windows.get_mut(&b) {
                    win.ui.items.selected.clear();
                }
            }
            Action::PortalUp { b, depth } => {
                let ctx = self.ctx.clone();
                if let Some(p) = self.portals.get_mut(&b) {
                    p.up_to(depth, &ctx);
                }
                if let Some(win) = self.windows.get_mut(&b) {
                    win.ui.items.selected.clear();
                }
            }
            Action::ApplyRules => {
                let store = self.store.clone();
                let tx = self.channels.reply_tx.clone();
                let ctx = self.ctx.clone();
                std::thread::spawn(move || {
                    let text = match ipc::request(&store, &Request::ApplyRules { all: false }) {
                        Ok(Response::Error { message }) => message,
                        Ok(_) => t!("box.rules_applied").to_owned(),
                        Err(e) => {
                            log::warn!("appliquer les règles : {e:#}");
                            t!("error.agent_unreachable").to_owned()
                        }
                    };
                    let _ = tx.send(text);
                    ctx.request_repaint();
                });
            }
            Action::DismissUnassigned => {
                self.overlay.dismiss_unassigned(self.state.unassigned.len());
            }
        }
        self.generation += 1;
    }

    /// Modifie une boîte puis enregistre.
    fn edit_box(&mut self, b: BoxId, edit: impl FnOnce(&mut DeskBox)) {
        if let Some(x) = self.state.find_box_mut(b) {
            edit(x);
            self.save(false);
        }
    }

    /// Point d'ancrage d'une confirmation : en haut au centre de la boîte.
    fn popup_anchor(&self, b: BoxId) -> egui::Pos2 {
        match self.windows.get(&b) {
            Some(win) => {
                let r = win.shown(&self.screens);
                pos2(r.x + (r.width - 340.0).max(0.0) / 2.0, r.y + TITLE_HEIGHT)
            }
            None => mouse_pos(),
        }
    }

    fn rescan_dir_of(&mut self, b: BoxId) {
        match self.portals.get_mut(&b) {
            Some(p) => p.rescan(),
            None => {
                self.rescan_desktop();
            }
        }
    }

    fn rename(&mut self, b: BoxId, old: &str, new: &str) {
        let Some(dir) = self.dir_of(b) else { return };
        let target = match ops::validate_new_name(dir.parent_of(old), old, new) {
            Ok(t) => t,
            Err(e) => {
                let text = match e {
                    ops::NameError::Empty => t!("box.rename_empty").to_owned(),
                    ops::NameError::Invalid => t!("box.rename_invalid").to_owned(),
                    ops::NameError::Exists => t!("box.rename_exists", name = new),
                };
                self.notice(b, text);
                return;
            }
        };
        let source = dir.join(old);
        if let Err(e) = std::fs::rename(&source, &target) {
            log::warn!("renommer {} : {e}", source.display());
            self.notice(b, t!("box.rename_failed"));
            return;
        }
        let new = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.icons.forget(&source);
        if !self.is_portal(b) {
            ops::rename_item(&mut self.state, old, &new);
            self.save(true);
        }
        if let Some(win) = self.windows.get_mut(&b) {
            win.ui.items.selected.clear();
            win.ui.items.selected.insert(new);
        }
        self.rescan_dir_of(b);
    }

    /// Grille libre de la boîte `b` : pose `names` sur la case sous la souris. Rend
    /// `false` si la boîte n'a pas de grille libre affichée.
    fn place_at_mouse(&mut self, b: BoxId, names: &[String]) -> bool {
        let Some(win) = self.windows.get(&b) else {
            return false;
        };
        let Some(geom) = win.grid else {
            return false;
        };
        let target = geom.cell_at(mouse_pos());
        let shown: Vec<String> = win
            .sections
            .iter()
            .flat_map(|s| s.items.iter().map(|i| i.name.clone()))
            .collect();
        let Some(x) = self.state.find_box_mut(b) else {
            return false;
        };
        if items::free_grid(x).is_none() {
            return false;
        }
        let tab = x.active_tab.min(x.tabs.len().saturating_sub(1));
        let shown: Vec<&str> = shown.iter().map(String::as_str).collect();
        tidy_core::grid::place(&mut x.tabs[tab].cells, &shown, geom.columns, names, target);
        true
    }

    /// Fichiers déposés dans une boîte. Ceux du bureau y sont rangés ; ceux venus
    /// d'ailleurs ne sont ni copiés ni déplacés (aucun fichier ne bouge).
    fn drop_files(&mut self, b: BoxId, paths: &[PathBuf], before: Option<String>) {
        if self.is_portal(b) {
            self.notice(b, t!("box.drop_portal"));
            return;
        }
        let dirs: Vec<PathBuf> = self
            .desktop_dir
            .iter()
            .chain(self.public_dir.iter())
            .cloned()
            .collect();
        let (names, outside) = split_desktop_items(paths, &dirs);
        if !names.is_empty() {
            let Some(x) = self.state.find_box(b) else {
                return;
            };
            let tab = x.active_tab;
            let manual = x.sort.key == SortKey::Manual && !x.sort.group_by_date;
            let before = before.filter(|n| manual && !names.contains(n));
            ops::assign_to_tab(&mut self.state, &names, b, tab, before.as_deref());
            self.place_at_mouse(b, &names);
            self.save(true);
            self.rescan_desktop();
        }
        if outside > 0 {
            self.notice(b, t!("box.drop_outside", n = outside));
        }
    }

    /// Fin d'un glisser interne : vers une autre boîte (rangé), dans la même boîte
    /// (ordre manuel), ailleurs (rien, avec une explication).
    fn end_drag(&mut self, from: BoxId, before: Option<String>) {
        let Some(drag) = self.drag.take() else { return };
        if drag.from != from {
            return;
        }
        let target = self.box_under(tidy_platform::current().mouse_location());
        match target {
            Some(to) if to == from => {
                if drag.portal {
                    return;
                }
                if self.place_at_mouse(from, &drag.names) {
                    self.save(false);
                    self.generation += 1;
                    return;
                }
                let Some(x) = self.state.find_box_mut(from) else {
                    return;
                };
                let manual = x.sort.key == SortKey::Manual && !x.sort.group_by_date;
                let Some(before) = before.filter(|n| manual && !drag.names.contains(n)) else {
                    return;
                };
                let tab = x.active_tab.min(x.tabs.len().saturating_sub(1));
                ops::move_in_tab(&mut x.tabs[tab], &drag.names, Some(&before));
                self.save(false);
            }
            Some(to) if self.is_portal(to) => self.notice(to, t!("box.drop_portal")),
            Some(to) => {
                if drag.portal {
                    self.notice(to, t!("box.drop_outside", n = drag.names.len()));
                    return;
                }
                let tab = self.state.find_box(to).map_or(0, |x| x.active_tab);
                ops::assign_to_tab(&mut self.state, &drag.names, to, tab, None);
                self.place_at_mouse(to, &drag.names);
                self.save(true);
            }
            None => {
                if !drag.portal {
                    self.notice(from, t!("box.drag_out_unavailable"));
                }
            }
        }
        self.generation += 1;
    }

    /// « Nouveau portail de dossier… » : choisir un dossier hors du bureau.
    fn new_portal(&mut self, near: BoxId) {
        let Some(path) = rfd::FileDialog::new()
            .set_title(t!("box.pick_folder"))
            .pick_folder()
        else {
            return;
        };
        match portal::check_folder(&path, self.desktop_dir.as_deref()) {
            Ok(()) => {
                let mut b = DeskBox::new(portal::display_name(&path));
                b.kind = BoxKind::Portal { path };
                let rect = self.rect_near(near);
                self.add_box(b, rect);
            }
            Err(PortalRefusal::OnDesktop) => self.notice(near, t!("box.portal_on_desktop")),
            Err(PortalRefusal::NotAFolder) => self.notice(near, t!("box.portal_unreadable")),
        }
    }
}

/// Noms des éléments du bureau parmi `paths`, et nombre de fichiers venus d'ailleurs.
fn split_desktop_items(paths: &[PathBuf], desktop: &[PathBuf]) -> (Vec<String>, usize) {
    let resolve = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let desktop: Vec<PathBuf> = desktop.iter().map(|d| resolve(d)).collect();
    let mut names = Vec::new();
    let mut outside = 0;
    for path in paths {
        let parent = path.parent().map(resolve);
        match (parent, path.file_name()) {
            (Some(p), Some(name)) if desktop.contains(&p) => {
                let name = name.to_string_lossy().into_owned();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            _ => outside += 1,
        }
    }
    (names, outside)
}

/// Ouvre la fenêtre principale à une page, par l'agent : il ramène devant celle qui
/// serait déjà ouverte au lieu d'en lancer une deuxième.
fn open_manager(page: &str) {
    let page = page.to_owned();
    std::thread::spawn(move || {
        let Ok(store) = Store::open_default() else {
            return;
        };
        if ipc::request(
            &store,
            &Request::OpenManager {
                page: Some(page.clone()),
            },
        )
        .is_err()
        {
            // Sans agent : lancement direct.
            let _ = std::env::current_exe().and_then(|exe| {
                std::process::Command::new(exe)
                    .args(["--manager", &page])
                    .spawn()
            });
        }
    });
}

fn mouse_pos() -> egui::Pos2 {
    let p = tidy_platform::current().mouse_location();
    pos2(p.x, p.y)
}

fn center(r: Rect) -> Point {
    Point {
        x: r.x + r.width / 2.0,
        y: r.y + r.height / 2.0,
    }
}

/// Deux cadres égaux à un demi-point près.
fn near(a: Rect, b: Rect) -> bool {
    (a.x - b.x).abs() <= 0.5
        && (a.y - b.y).abs() <= 0.5
        && (a.width - b.width).abs() <= 0.5
        && (a.height - b.height).abs() <= 0.5
}

fn same_size(a: Rect, b: Rect) -> bool {
    (a.width - b.width).abs() <= 0.5 && (a.height - b.height).abs() <= 0.5
}

/// Bords tirés pendant un redimensionnement de `before` à `after`.
fn resized_edges(before: Rect, after: Rect) -> Edges {
    Edges {
        left: (after.x - before.x).abs() > 0.5,
        top: (after.y - before.y).abs() > 0.5,
        right: (after.right() - before.right()).abs() > 0.5,
        bottom: (after.bottom() - before.bottom()).abs() > 0.5,
    }
}

/// Demande à la fenêtre de prendre `target`, et attend qu'elle l'ait pris avant de
/// relire sa position.
fn move_window(ctx: &egui::Context, win: &mut BoxWindow, target: Rect) {
    ctx.send_viewport_cmd_to(
        win.viewport,
        ViewportCommand::OuterPosition(pos2(target.x, target.y)),
    );
    ctx.send_viewport_cmd_to(
        win.viewport,
        ViewportCommand::InnerSize(egui::vec2(target.width, target.height)),
    );
    win.expect = Some((target, Instant::now()));
    ctx.request_repaint();
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

/// Place par défaut de la `i`-ième boîte sans position : en colonnes depuis le coin en
/// haut à droite de l'écran principal, sans chevauchement (écart de 10 points).
fn default_rect(screens: &[Screen], i: usize) -> Rect {
    let v = screens
        .iter()
        .find(|s| s.primary)
        .unwrap_or(&screens[0])
        .visible_frame;
    let (w, h) = (NEW_BOX_SIZE[0], NEW_BOX_SIZE[1]);
    let (margin, gap) = (40.0, 10.0);
    let rows = (((v.height - 2.0 * margin + gap) / (h + gap)).floor() as usize).max(1);
    let (col, row) = ((i / rows) as f32, (i % rows) as f32);
    Rect::new(
        v.right() - margin - w - col * (w + gap),
        v.y + margin + row * (h + gap),
        w,
        h,
    )
    .clamped_to(&v)
}

fn box_builder(win: &BoxWindow) -> ViewportBuilder {
    let min = if win.rolled {
        [boxwin::MIN_SIZE[0], TITLE_HEIGHT]
    } else {
        boxwin::MIN_SIZE
    };
    ViewportBuilder::default()
        .with_title(&win.title)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(!win.rolled)
        .with_has_shadow(false)
        .with_taskbar(false)
        .with_position([win.spawn.x, win.spawn.y])
        .with_inner_size([win.spawn.width, win.spawn.height])
        .with_min_inner_size(min)
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
        self.icons.receive(&ctx);
        self.icons.sweep();
        let mut out: Vec<Action> = Vec::new();
        let mut pressed = false;
        let mut released = false;
        let drop_target = self
            .drag
            .as_ref()
            .and_then(|_| self.box_under(tidy_platform::current().mouse_location()));
        let ids: Vec<BoxId> = self.state.boxes.iter().map(|b| b.id).collect();
        for id in ids {
            self.refresh_sections(id);
            self.apply_roll(id);
            let Some(b) = self.state.find_box(id) else {
                continue;
            };
            let look = self.look(b);
            let visible = !self.hidden || b.exclude_from_quick_hide;
            let portal_view = self.portals.get(&id).map(|p| PortalView {
                crumbs: p.crumbs(),
                error: p.error,
            });
            let dir = self.dir_of(id);
            let single_click = self.settings.quick_actions.single_click_open;
            let Some(win) = self.windows.get_mut(&id) else {
                continue;
            };
            let builder = box_builder(win).with_visible(visible);
            let viewport = win.viewport;
            let sections = std::mem::take(&mut win.sections);
            let view = BoxView {
                b,
                look: &look,
                sections: &sections,
                dir: dir.as_ref(),
                portal: portal_view,
                rolled: win.rolled,
                drop_target: drop_target == Some(id)
                    && self.drag.as_ref().is_some_and(|d| d.from != id),
                single_click,
            };
            let icons = &mut self.icons;
            let box_ui = &mut win.ui;
            let (res, outer) = ctx.show_viewport_immediate(viewport, builder, |ui, _class| {
                let res = boxwin::draw(ui, &view, box_ui, icons, &mut out);
                (res, ui.ctx().input(|i| i.viewport().outer_rect))
            });
            drop(view);
            win.sections = sections;
            win.grid = res
                .grid
                .zip(outer)
                .map(|(g, o)| g.translated(o.min.to_vec2()));
            pressed |= res.pressed;
            released |= res.released;
            if res.hovered {
                win.last_hover = Instant::now();
            } else if win.hovered {
                ctx.request_repaint_after(roll::AUTO_ROLL_DELAY);
            }
            win.hovered = res.hovered;
            if !win.placed {
                let shown = win.shown(&self.screens);
                win.placed = tidy_platform::current().make_desktop_surface_titled(
                    &win.title,
                    shown,
                    SurfaceLevel::DesktopIcons,
                );
                if win.placed && win.raised {
                    tidy_platform::current().raise_desktop_surface(&win.title, true);
                }
                ctx.request_repaint();
            }
            self.track_window(id, outer, res.shift);
            if self
                .windows
                .get(&id)
                .is_some_and(|w| w.moved_at.is_some() || w.expect.is_some())
            {
                ctx.request_repaint_after(SAVE_DELAY);
            }
        }
        if self.drag.is_some() {
            // Suivre la souris pour montrer la boîte visée.
            ctx.request_repaint_after(Duration::from_millis(50));
            // Bouton relâché sans fin de glisser (l'élément glissé a disparu entre-temps) :
            // terminer quand même, sinon la boîte visée resterait en surbrillance.
            if released && !out.iter().any(|a| matches!(a, Action::EndDrag { .. })) {
                let from = self.drag.as_ref().map(|d| d.from);
                if let Some(from) = from {
                    out.push(Action::EndDrag {
                        b: from,
                        before: None,
                    });
                }
            }
        }

        // Widgets.
        let mut widget_commands = Vec::new();
        self.widgets.show(
            &ctx,
            &self.state,
            &self.screens,
            self.hidden,
            &mut widget_commands,
        );
        if !widget_commands.is_empty() {
            for command in widget_commands {
                self.state.apply(command);
            }
            self.save(false);
        }

        // Fenêtres flottantes.
        if pressed && !out.iter().any(|a| matches!(a, Action::BoxMenu { .. })) {
            self.overlay.close_popup();
        }
        let (state, settings) = (&self.state, &self.settings);
        self.overlay.show_popup(
            &ctx,
            &self.screens,
            |b, page| Self::box_menu(state, settings, b, page),
            &mut out,
        );
        self.overlay.show_unassigned(
            &ctx,
            &self.screens,
            self.state.unassigned.len(),
            self.hidden,
            &mut out,
        );
        self.overlay.show_draw_offer(&ctx, &self.screens, &mut out);

        for action in out {
            self.apply(action);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_desktop_items_are_boxed() {
        let dir = tempfile::tempdir().unwrap();
        let desktop = dir.path().join("Bureau");
        let other = dir.path().join("Documents");
        std::fs::create_dir_all(&desktop).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        for p in [
            desktop.join("a.pdf"),
            desktop.join("b.png"),
            other.join("c.txt"),
        ] {
            std::fs::write(p, b"x").unwrap();
        }
        let (names, outside) = split_desktop_items(
            &[
                desktop.join("a.pdf"),
                other.join("c.txt"),
                desktop.join("b.png"),
            ],
            std::slice::from_ref(&desktop),
        );
        assert_eq!(names, ["a.pdf", "b.png"]);
        assert_eq!(outside, 1);
        // Bureau public : ses éléments sont aussi du bureau.
        let (names, outside) =
            split_desktop_items(&[other.join("c.txt")], &[desktop.clone(), other.clone()]);
        assert_eq!(names, ["c.txt"]);
        assert_eq!(outside, 0);
        let (names, outside) = split_desktop_items(&[desktop.join("a.pdf")], &[]);
        assert!(names.is_empty());
        assert_eq!(outside, 1);
    }

    #[test]
    fn rolled_window_keeps_its_full_frame() {
        let mut win = BoxWindow {
            viewport: ViewportId::from_hash_of("essai"),
            title: String::new(),
            spawn: Rect::default(),
            current: Rect::new(100.0, 600.0, 300.0, 200.0),
            placed: true,
            moved_at: None,
            move_origin: None,
            shift: false,
            expect: None,
            rolled: true,
            anchored_bottom: true,
            hovered: false,
            last_hover: Instant::now(),
            raised: false,
            ui: BoxUi::default(),
            sections: Vec::new(),
            sections_key: (0, 0),
            grid: None,
        };
        assert_eq!(
            win.shown(&[]),
            Rect::new(100.0, 800.0 - TITLE_HEIGHT, 300.0, TITLE_HEIGHT)
        );
        win.anchored_bottom = false;
        assert_eq!(win.shown(&[]), Rect::new(100.0, 600.0, 300.0, TITLE_HEIGHT));
    }
}
