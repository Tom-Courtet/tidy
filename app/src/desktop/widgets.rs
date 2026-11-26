//! Widgets posés sur le bureau (lots H et I) : une fenêtre par instance de
//! `DesktopState::widgets`, au niveau des boîtes, avec un cadre commun (déplacer,
//! geler, masquer avec le bureau, réglages, supprimer). Le contenu vient du registre de
//! `tidy-widgets`.
//!
//! Le processus du bureau reste le seul écrivain de `desktop.json` : l'hôte rend des
//! [`DesktopCommand`] (position, réglages, suppression), appliquées par `Desktop`.
//!
//! - **Fenêtre** : comme une boîte (ADR 011), posée juste au-dessus des icônes, sous
//!   les applis, immobile pendant Win+D ; le système la déplace (`StartDrag`) et la
//!   redimensionne (`BeginResize`, coin en bas à droite) ; la position finale est
//!   mémorisée par configuration d'écrans. Un widget gelé ne bouge plus.
//! - **Cadre** : carte sombre ; au survol (ou toujours, selon le réglage commun), une
//!   barre avec la poignée et les boutons réglages, épingler, geler, supprimer. Clic
//!   droit : réglages.
//! - **Réglages** : petite fenêtre flottante à côté du widget (réglages propres, puis
//!   réglages communs) ; la suppression y demande confirmation.
//! - **Épinglé au premier plan** : la fenêtre passe au-dessus des applis ; l'état est
//!   gardé dans les réglages de l'instance ([`PINNED_KEY`]).
//! - **Repos** : rien ne se redessine, sauf ce que demande un widget
//!   ([`Widget::repaint_after`]) ; les réglages changés sont enregistrés une seconde
//!   après la dernière modification.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::viewport::ResizeDirection;
use egui::{
    Align2, CornerRadius, CursorIcon, FontId, Key, Modifiers, RichText, Sense, Stroke, StrokeKind,
    ViewportBuilder, ViewportCommand, ViewportId, WindowLevel, pos2, vec2,
};
use serde_json::Value;
use tidy_core::geometry::{Point, Rect};
use tidy_core::layouts::resolve_placement;
use tidy_core::model::WidgetInstance;
use tidy_core::screen::Screen;
use tidy_core::settings::ResolutionPolicy;
use tidy_core::settings::widgets::WidgetSettings;
use tidy_core::{DesktopCommand, DesktopState, Settings, Store};
use tidy_platform::SurfaceLevel;
use tidy_ui::{icons, t, theme, widgets};
use tidy_widgets::{PINNED_KEY, Widget, WidgetAction, WidgetCx};
use uuid::Uuid;

use crate::ipc::{self, Request};

/// Hauteur de la barre du cadre (poignée et boutons).
const BAR: f32 = 22.0;
/// Marge autour du contenu.
const PAD: f32 = 6.0;
/// Taille minimale d'une fenêtre de widget.
const MIN_SIZE: [f32; 2] = [110.0, 70.0];
/// Délai après le dernier mouvement d'une fenêtre avant d'enregistrer sa place.
const SAVE_DELAY: Duration = Duration::from_millis(400);
/// Délai après la dernière modification des réglages avant de les enregistrer.
const SETTINGS_DELAY: Duration = Duration::from_secs(1);
/// Au-delà, on cesse d'attendre que la fenêtre prenne le cadre demandé.
const EXPECT_TIMEOUT: Duration = Duration::from_secs(1);
/// Taille de la fenêtre des réglages.
const PANEL_SIZE: [f32; 2] = [340.0, 500.0];

/// Taille de la fenêtre pour un contenu de `content` points.
fn frame_size(content: [f32; 2]) -> [f32; 2] {
    [
        (content[0] + 2.0 * PAD).max(MIN_SIZE[0]),
        (content[1] + BAR + PAD).max(MIN_SIZE[1]),
    ]
}

/// Réglages enregistrés d'un widget, avec l'épinglage de l'hôte.
fn merged_settings(widget: &dyn Widget, pinned: bool) -> Value {
    let mut value = widget.save();
    if let Value::Object(map) = &mut value {
        if pinned {
            map.insert(PINNED_KEY.into(), Value::Bool(true));
        } else {
            map.remove(PINNED_KEY);
        }
    }
    value
}

fn pinned_in(settings: &Value) -> bool {
    settings
        .get(PINNED_KEY)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Fenêtre ouverte à côté d'un widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Settings,
    ConfirmDelete,
}

/// Bouton de la barre du cadre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarAction {
    Settings,
    Pin,
    Freeze,
    Delete,
}

struct WidgetWindow {
    viewport: ViewportId,
    title: String,
    kind: String,
    name: &'static str,
    icon: &'static str,
    widget: Box<dyn Widget>,
    /// Réglages tels qu'enregistrés : un écart vient d'ailleurs (fenêtre principale).
    saved: Value,
    pinned: bool,
    /// Épinglage appliqué à la fenêtre native.
    raised: bool,
    /// Cadre de création (constant, pour qu'egui ne replace pas la fenêtre).
    spawn: Rect,
    /// Cadre actuel, relu sur la fenêtre.
    current: Rect,
    /// Fenêtre native réglée (niveau du bureau…).
    placed: bool,
    moved_at: Option<Instant>,
    /// Cadre demandé à la fenêtre : on ne relit pas sa position avant qu'elle l'ait pris.
    expect: Option<(Rect, Instant)>,
    /// Réglages modifiés, pas encore enregistrés.
    dirty_since: Option<Instant>,
    panel: Option<Panel>,
    /// Le panneau a été ouvert devant (une seule fois).
    panel_focused: bool,
}

/// Hôte des widgets du processus du bureau.
pub struct WidgetHost {
    store: Store,
    data_dir: PathBuf,
    settings: WidgetSettings,
    windows: HashMap<Uuid, WidgetWindow>,
    screens: Vec<Screen>,
    /// Types inconnus déjà signalés dans le journal.
    unknown: HashSet<String>,
    /// Bureau masqué par le mode concentration d'un pomodoro (à rendre ensuite).
    focus_hidden: bool,
    /// Réglage du changement de résolution (`LayoutSettings::on_resolution_change`).
    policy: ResolutionPolicy,
}

/// Modifications d'instances de l'image en cours (une commande par instance).
#[derive(Default)]
struct Edits {
    changed: Vec<WidgetInstance>,
    removed: Vec<Uuid>,
}

impl Edits {
    fn of<'a>(&'a mut self, inst: &WidgetInstance) -> &'a mut WidgetInstance {
        match self.changed.iter().position(|w| w.id == inst.id) {
            Some(i) => &mut self.changed[i],
            None => {
                self.changed.push(inst.clone());
                self.changed.last_mut().expect("ajouté juste avant")
            }
        }
    }
}

impl WidgetHost {
    pub fn new(store: &Store) -> Self {
        let settings: Settings = store.load();
        Self {
            store: store.clone(),
            data_dir: store.root().to_path_buf(),
            policy: settings.layout.on_resolution_change,
            settings: settings.widgets,
            windows: HashMap::new(),
            screens: Vec::new(),
            unknown: HashSet::new(),
            focus_hidden: false,
        }
    }

    /// Les réglages ont changé (`Event::SettingsChanged`).
    pub fn settings_changed(&mut self, settings: &Settings) {
        self.settings = settings.widgets.clone();
        self.policy = settings.layout.on_resolution_change;
    }

    /// Instantané ou disposition restaurés : chaque widget reprend la place enregistrée
    /// (même avec « ne rien faire » au changement de résolution).
    pub fn restored(&mut self, ctx: &egui::Context, state: &DesktopState) {
        let policy = match self.policy {
            ResolutionPolicy::Nothing => ResolutionPolicy::RememberPerResolution,
            policy => policy,
        };
        self.place_all(ctx, state, policy);
    }

    /// Dessine une fenêtre par widget de `state` (créée au premier passage, oubliée
    /// quand le widget disparaît). `hidden` : bureau masqué (sauf widgets exclus).
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        state: &DesktopState,
        screens: &[Screen],
        hidden: bool,
        out: &mut Vec<DesktopCommand>,
    ) {
        if screens.is_empty() {
            return;
        }
        let mut edits = Edits::default();
        if self.screens.as_slice() != screens {
            let first = self.screens.is_empty();
            self.screens = screens.to_vec();
            if !first {
                self.screens_changed(ctx, state);
            }
        }
        self.windows
            .retain(|id, _| state.widgets.iter().any(|w| w.id == *id));
        if !hidden {
            self.focus_hidden = false;
        }
        let mut repaint: Option<Duration> = None;
        let mut soon = |d: Duration| repaint = Some(repaint.map_or(d, |r: Duration| r.min(d)));
        for inst in &state.widgets {
            if !self.windows.contains_key(&inst.id) && !self.create(state, inst, &mut edits) {
                continue;
            }
            let actions = self.frame(ctx, inst, hidden, &mut edits, &mut soon);
            for action in actions {
                self.run(inst, action, hidden, &mut edits);
            }
        }
        if let Some(d) = repaint {
            ctx.request_repaint_after(d);
        }
        for id in edits.removed {
            edits.changed.retain(|w| w.id != id);
            out.push(DesktopCommand::RemoveWidget { id });
        }
        for w in edits.changed {
            out.push(DesktopCommand::UpsertWidget { w });
        }
    }

    /// Crée la fenêtre d'un widget. `false` : type inconnu (widget d'une version plus
    /// récente, ou d'un lot absent) ; l'instance est gardée telle quelle.
    fn create(&mut self, state: &DesktopState, inst: &WidgetInstance, edits: &mut Edits) -> bool {
        let Some(kind) = tidy_widgets::kind(&inst.kind) else {
            if self.unknown.insert(inst.kind.clone()) {
                log::warn!("widget de type inconnu : {}", inst.kind);
            }
            return false;
        };
        let widget = (kind.create)(&inst.settings);
        let pinned = pinned_in(&inst.settings);
        let saved = merged_settings(widget.as_ref(), pinned);
        if saved != inst.settings {
            // Réglages complétés (valeurs par défaut, identifiant d'une note).
            edits.of(inst).settings = saved.clone();
        }
        let mut rect = match inst.placement.resolve(&self.screens) {
            Some((_, r)) => r,
            None => {
                let occupied: Vec<Rect> = state
                    .boxes
                    .iter()
                    .filter_map(|b| b.placement.resolve(&self.screens).map(|(_, r)| r))
                    .chain(self.windows.values().map(|w| w.current))
                    .collect();
                let area = primary(&self.screens).visible_frame;
                free_spot(area, &occupied, kind.default_size)
            }
        };
        if let Some(size) = widget.preferred_size() {
            let [w, h] = frame_size(size);
            rect = Rect::new(rect.x, rect.y, w, h);
        }
        let screen = screen_at(&self.screens, center(rect));
        rect = rect.clamped_to(&screen.visible_frame);
        if inst.placement.resolve(&self.screens) != Some((screen.id.clone(), rect)) {
            edits
                .of(inst)
                .placement
                .remember(&self.screens, &screen, rect);
        }
        let short: String = inst.id.simple().to_string().chars().take(8).collect();
        self.windows.insert(
            inst.id,
            WidgetWindow {
                viewport: ViewportId::from_hash_of(("tidy-widget", inst.id)),
                title: format!("Tidy · widget {short}"),
                kind: inst.kind.clone(),
                name: tidy_ui::i18n::tr(kind.name_key),
                icon: kind.icon,
                widget,
                saved,
                pinned,
                raised: false,
                spawn: rect,
                current: rect,
                placed: false,
                moved_at: None,
                expect: None,
                dirty_since: None,
                panel: None,
                panel_focused: false,
            },
        );
        true
    }

    /// Écrans changés : chaque widget reprend sa place selon le réglage du changement
    /// de résolution, comme les boîtes.
    fn screens_changed(&mut self, ctx: &egui::Context, state: &DesktopState) {
        self.place_all(ctx, state, self.policy);
    }

    fn place_all(&mut self, ctx: &egui::Context, state: &DesktopState, policy: ResolutionPolicy) {
        if self.screens.is_empty() {
            return;
        }
        for inst in &state.widgets {
            let Some(win) = self.windows.get_mut(&inst.id) else {
                continue;
            };
            if policy == ResolutionPolicy::Nothing
                && self
                    .screens
                    .iter()
                    .any(|s| s.frame.contains(center(win.current)))
            {
                continue;
            }
            let rect = match resolve_placement(&inst.placement, &self.screens, policy) {
                Some((_, r)) => r,
                None => win
                    .current
                    .clamped_to(&primary(&self.screens).visible_frame),
            };
            win.current = rect;
            move_window(ctx, win, rect);
        }
    }

    /// Dessine la fenêtre d'un widget (et son panneau) ; rend les demandes du widget.
    fn frame(
        &mut self,
        ctx: &egui::Context,
        inst: &WidgetInstance,
        hidden: bool,
        edits: &mut Edits,
        soon: &mut impl FnMut(Duration),
    ) -> Vec<WidgetAction> {
        let data_dir = self.data_dir.clone();
        let look = self.settings.clone();
        let screens = self.screens.clone();
        let Some(win) = self.windows.get_mut(&inst.id) else {
            return Vec::new();
        };
        // Réglages changés ailleurs (fenêtre principale) : recréer le widget.
        if win.dirty_since.is_none() && inst.settings != win.saved {
            if let Some(kind) = tidy_widgets::kind(&inst.kind) {
                win.widget = (kind.create)(&inst.settings);
            }
            win.saved = inst.settings.clone();
            win.pinned = pinned_in(&inst.settings);
        }
        let visible = !hidden
            || inst.exclude_from_quick_hide
            || !look.hide_with_desktop
            || win.widget.keep_visible();

        // Taille voulue par le widget (horloge : taille, fuseaux).
        if let Some(size) = win.widget.preferred_size() {
            let [w, h] = frame_size(size);
            if ((w - win.current.width).abs() > 1.5 || (h - win.current.height).abs() > 1.5)
                && win.expect.is_none()
            {
                let screen = screen_at(&screens, center(win.current));
                let target =
                    Rect::new(win.current.x, win.current.y, w, h).clamped_to(&screen.visible_frame);
                win.current = target;
                move_window(ctx, win, target);
                edits.of(inst).placement.remember(&screens, &screen, target);
            }
        }

        let builder = ViewportBuilder::default()
            .with_title(&win.title)
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(true)
            .with_has_shadow(false)
            .with_taskbar(false)
            .with_position([win.spawn.x, win.spawn.y])
            .with_inner_size([win.spawn.width, win.spawn.height])
            .with_min_inner_size(MIN_SIZE)
            .with_visible(visible);
        let frame_look = FrameLook {
            frozen: inst.frozen,
            pinned: win.pinned,
            bar_always: !look.frame_on_hover,
            resizable: !inst.frozen && win.widget.preferred_size().is_none(),
        };
        let mut cx = WidgetCx::new(data_dir.clone());
        let widget = &mut win.widget;
        let (res, outer) = ctx.show_viewport_immediate(win.viewport, builder, |ui, _| {
            let res = draw_frame(ui, widget.as_mut(), &mut cx, &frame_look);
            (res, ui.ctx().input(|i| i.viewport().outer_rect))
        });

        // Fenêtre native posée sur le bureau, puis épinglée si besoin.
        let platform = tidy_platform::current();
        if !win.placed && visible {
            win.placed = platform.make_desktop_surface_titled(
                &win.title,
                win.current,
                SurfaceLevel::DesktopIcons,
            );
            win.raised = false;
            ctx.request_repaint();
        }
        if win.placed && win.pinned != win.raised {
            platform.raise_desktop_surface(&win.title, win.pinned);
            win.raised = win.pinned;
        }

        // Place : relue sur la fenêtre, enregistrée une fois stable.
        if track(win, outer) {
            let screen = screen_at(&screens, center(win.current));
            edits
                .of(inst)
                .placement
                .remember(&screens, &screen, win.current);
        }
        if win.moved_at.is_some() || win.expect.is_some() {
            soon(SAVE_DELAY);
        }

        // Barre du cadre.
        match res.bar {
            Some(BarAction::Settings) => {
                win.panel = match win.panel {
                    Some(Panel::Settings) => None,
                    _ => Some(Panel::Settings),
                };
                win.panel_focused = false;
            }
            Some(BarAction::Delete) => {
                win.panel = Some(Panel::ConfirmDelete);
                win.panel_focused = false;
            }
            Some(BarAction::Pin) => {
                win.pinned = !win.pinned;
                let settings = merged_settings(win.widget.as_ref(), win.pinned);
                win.saved = settings.clone();
                edits.of(inst).settings = settings;
            }
            Some(BarAction::Freeze) => {
                let e = edits.of(inst);
                e.frozen = !e.frozen;
            }
            None => {}
        }

        // Panneau des réglages, ou confirmation de suppression.
        if win.panel.is_some() {
            let screen = screen_at(&screens, center(win.current));
            let pos = panel_position(win.current, PANEL_SIZE, screen.visible_frame);
            let mut state = PanelState {
                frozen: edits
                    .changed
                    .iter()
                    .find(|w| w.id == inst.id)
                    .map_or(inst.frozen, |w| w.frozen),
                exclude: inst.exclude_from_quick_hide,
                pinned: win.pinned,
                settings_changed: false,
                close: false,
                ask_delete: false,
                delete: false,
            };
            let panel_id = ViewportId::from_hash_of(("tidy-widget-réglages", inst.id));
            let builder = ViewportBuilder::default()
                .with_title(format!("Tidy · {}", win.name))
                .with_decorations(false)
                .with_transparent(true)
                .with_resizable(false)
                .with_has_shadow(true)
                .with_taskbar(false)
                .with_window_level(WindowLevel::AlwaysOnTop)
                .with_active(true)
                .with_position([pos.x, pos.y])
                .with_inner_size(PANEL_SIZE);
            let mode = win.panel.unwrap_or(Panel::Settings);
            let (name, icon, kind) = (win.name, win.icon, win.kind.as_str());
            let widget = &mut win.widget;
            ctx.show_viewport_immediate(panel_id, builder, |ui, _| {
                draw_panel(
                    ui,
                    mode,
                    name,
                    icon,
                    kind,
                    widget.as_mut(),
                    &mut cx,
                    &mut state,
                );
            });
            if !win.panel_focused {
                ctx.send_viewport_cmd_to(panel_id, ViewportCommand::Focus);
                win.panel_focused = true;
            }
            if state.settings_changed {
                cx.changed = true;
            }
            if state.pinned != win.pinned {
                win.pinned = state.pinned;
                let settings = merged_settings(win.widget.as_ref(), win.pinned);
                win.saved = settings.clone();
                edits.of(inst).settings = settings;
            }
            if state.frozen
                != edits
                    .changed
                    .iter()
                    .find(|w| w.id == inst.id)
                    .map_or(inst.frozen, |w| w.frozen)
            {
                edits.of(inst).frozen = state.frozen;
            }
            if state.exclude != inst.exclude_from_quick_hide {
                edits.of(inst).exclude_from_quick_hide = state.exclude;
            }
            if state.delete {
                edits.removed.push(inst.id);
                win.panel = None;
            } else if state.ask_delete {
                win.panel = Some(Panel::ConfirmDelete);
            } else if state.close {
                win.panel = None;
            }
        }

        // Réglages : enregistrés une seconde après la dernière modification.
        if cx.changed {
            win.dirty_since = Some(Instant::now());
        }
        if let Some(since) = win.dirty_since {
            if since.elapsed() >= SETTINGS_DELAY {
                let settings = merged_settings(win.widget.as_ref(), win.pinned);
                win.saved = settings.clone();
                win.dirty_since = None;
                if settings != inst.settings {
                    edits.of(inst).settings = settings;
                }
            } else {
                soon(SETTINGS_DELAY.saturating_sub(since.elapsed()));
            }
        }
        if let Some(d) = win.widget.repaint_after() {
            soon(d);
        }
        cx.actions
    }

    /// Exécute une demande d'un widget.
    fn run(
        &mut self,
        inst: &WidgetInstance,
        action: WidgetAction,
        hidden: bool,
        edits: &mut Edits,
    ) {
        let platform = tidy_platform::current();
        match action {
            WidgetAction::Open(target) => {
                if let Err(e) = platform.open(Path::new(&target)) {
                    log::warn!("widget {} : ouverture de {target} : {e:#}", inst.kind);
                }
            }
            WidgetAction::Notify { title, body } => {
                self.notify(&title, &body, true);
            }
            WidgetAction::Alert { title, body, sound } => {
                self.notify(&title, &body, sound);
            }
            WidgetAction::Module {
                module,
                action,
                data,
            } => request_later(
                &self.store,
                Request::Module {
                    module,
                    action,
                    data,
                },
            ),
            WidgetAction::PinOnTop(on) => {
                if let Some(win) = self.windows.get_mut(&inst.id)
                    && win.pinned != on
                {
                    win.pinned = on;
                    let settings = merged_settings(win.widget.as_ref(), on);
                    win.saved = settings.clone();
                    edits.of(inst).settings = settings;
                }
            }
            WidgetAction::OpenManager(page) => super::open_manager(&page),
            WidgetAction::Launch { program, args } => {
                if let Err(e) = std::process::Command::new(&program).args(&args).spawn() {
                    log::warn!("widget {} : lancement de {program} : {e}", inst.kind);
                }
            }
            WidgetAction::HideDesktop(hide) => {
                // Ne rendre le bureau que si c'est le mode concentration qui l'a masqué.
                let toggle = if hide {
                    !hidden
                } else {
                    hidden && self.focus_hidden
                };
                if toggle {
                    self.focus_hidden = hide;
                    request_later(&self.store, Request::ToggleDesktop);
                }
            }
        }
    }

    fn notify(&self, title: &str, body: &str, sound: bool) {
        let sound = sound && self.settings.notification_sound;
        if !tidy_platform::current().notify(title, body, sound) {
            log::info!("notification (indisponible ici) : {title} · {body}");
        }
    }
}

/// Envoie une requête à l'agent sur un fil (la boucle ne doit pas attendre).
fn request_later(store: &Store, req: Request) {
    let store = store.clone();
    let spawned = std::thread::Builder::new()
        .name("tidy-widget-requête".into())
        .spawn(move || {
            if let Err(e) = ipc::request(&store, &req) {
                log::warn!("requête d'un widget ({req:?}) : {e:#}");
            }
        });
    if let Err(e) = spawned {
        log::warn!("fil de requête : {e}");
    }
}

/// Relit le cadre de la fenêtre. `true` quand un déplacement vient de se terminer
/// (cadre stable depuis [`SAVE_DELAY`]) : à enregistrer.
fn track(win: &mut WidgetWindow, outer: Option<egui::Rect>) -> bool {
    if let Some(r) = outer {
        let rect = Rect::new(r.min.x, r.min.y, r.width(), r.height());
        if let Some((expected, since)) = win.expect
            && (near(rect, expected) || since.elapsed() > EXPECT_TIMEOUT)
        {
            win.expect = None;
        }
        if win.expect.is_none() && win.placed && !near(rect, win.current) {
            win.current = rect;
            win.moved_at = Some(Instant::now());
        }
    }
    if win.moved_at.is_some_and(|t| t.elapsed() >= SAVE_DELAY) {
        win.moved_at = None;
        return true;
    }
    false
}

/// Demande à la fenêtre de prendre `target`, et attend qu'elle l'ait pris avant de
/// relire sa position.
fn move_window(ctx: &egui::Context, win: &mut WidgetWindow, target: Rect) {
    ctx.send_viewport_cmd_to(
        win.viewport,
        ViewportCommand::OuterPosition(pos2(target.x, target.y)),
    );
    ctx.send_viewport_cmd_to(
        win.viewport,
        ViewportCommand::InnerSize(vec2(target.width, target.height)),
    );
    win.expect = Some((target, Instant::now()));
    ctx.request_repaint();
}

/// Première place libre de `area` pour un widget de `size`, en colonnes depuis le coin
/// en haut à gauche (les boîtes se rangent depuis la droite). Sans place libre : en
/// cascade depuis le coin.
pub fn free_spot(area: Rect, occupied: &[Rect], size: [f32; 2]) -> Rect {
    let (margin, gap, step) = (40.0, 10.0, 20.0);
    let [w, h] = size;
    let fits = |r: &Rect| {
        let grown = Rect::new(
            r.x - gap,
            r.y - gap,
            r.width + 2.0 * gap,
            r.height + 2.0 * gap,
        );
        !occupied.iter().any(|o| o.intersects(&grown))
    };
    let mut x = area.x + margin;
    while x + w <= area.right() - margin {
        let mut y = area.y + margin;
        while y + h <= area.bottom() - margin {
            let r = Rect::new(x, y, w, h);
            if fits(&r) {
                return r;
            }
            y += step;
        }
        x += step;
    }
    let n = occupied.len() as f32;
    Rect::new(area.x + margin + 24.0 * n, area.y + margin + 24.0 * n, w, h).clamped_to(&area)
}

/// Place de la fenêtre des réglages : à droite du widget, sinon à gauche ; dans
/// l'écran.
fn panel_position(widget: Rect, size: [f32; 2], bounds: Rect) -> Point {
    let [w, h] = size;
    let x = if widget.right() + 8.0 + w <= bounds.right() {
        widget.right() + 8.0
    } else {
        widget.x - 8.0 - w
    };
    let r = Rect::new(x, widget.y, w, h).clamped_to(&bounds);
    Point { x: r.x, y: r.y }
}

fn primary(screens: &[Screen]) -> &Screen {
    screens.iter().find(|s| s.primary).unwrap_or(&screens[0])
}

/// Écran qui contient `p`, sinon l'écran principal.
fn screen_at(screens: &[Screen], p: Point) -> Screen {
    screens
        .iter()
        .find(|s| s.frame.contains(p))
        .unwrap_or_else(|| primary(screens))
        .clone()
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

/// Allure du cadre commun.
struct FrameLook {
    frozen: bool,
    pinned: bool,
    /// Barre visible même sans survol.
    bar_always: bool,
    /// Coin de redimensionnement (taille libre, widget non gelé).
    resizable: bool,
}

struct FrameOut {
    bar: Option<BarAction>,
}

/// Cadre commun et contenu du widget.
fn draw_frame(
    ui: &mut egui::Ui,
    widget: &mut dyn Widget,
    cx: &mut WidgetCx,
    look: &FrameLook,
) -> FrameOut {
    let ctx = ui.ctx().clone();
    let rect = ui.max_rect();
    let hovered = ctx.input(|i| i.pointer.hover_pos().is_some());
    let painter = ui.painter().clone();
    let radius = CornerRadius::same(12);
    painter.rect_filled(rect, radius, theme::SURFACE);
    let border = if look.pinned {
        theme::ACCENT
    } else {
        theme::BORDER
    };
    painter.rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);
    let mut out = FrameOut { bar: None };

    // Fond : glisser pour déplacer, clic droit pour les réglages. Ajouté en premier :
    // le contenu passe devant.
    let background = ui.interact(rect, ui.scope_id().with("fond"), Sense::click_and_drag());
    if background.drag_started() && !look.frozen {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if background.secondary_clicked() {
        out.bar = Some(BarAction::Settings);
    }

    // Contenu.
    let content = egui::Rect::from_min_max(
        pos2(rect.left() + PAD, rect.top() + BAR),
        pos2(rect.right() - PAD, rect.bottom() - PAD),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content));
    child.set_clip_rect(content.intersect(ui.clip_rect()));
    widget.ui(&mut child, cx);

    // Barre : poignée et boutons.
    if hovered || look.bar_always {
        let y = rect.top() + BAR / 2.0 + 1.0;
        let (grip, grip_tip) = if look.frozen {
            (icons::LOCK, t!("widgets.frozen"))
        } else {
            (icons::GRIP, t!("widgets.move"))
        };
        let grip_rect = egui::Rect::from_center_size(pos2(rect.left() + 14.0, y), vec2(18.0, 18.0));
        painter.text(
            grip_rect.center(),
            Align2::CENTER_CENTER,
            grip,
            FontId::proportional(theme::SIZE_S),
            theme::TEXT_DIM,
        );
        let _ = ui
            .interact(grip_rect, ui.scope_id().with("poignée"), Sense::hover())
            .on_hover_text(grip_tip);
        let buttons = [
            (icons::CLOSE, t!("widgets.delete"), BarAction::Delete, false),
            (
                if look.frozen {
                    icons::UNLOCK
                } else {
                    icons::LOCK
                },
                if look.frozen {
                    t!("widgets.unfreeze")
                } else {
                    t!("widgets.freeze")
                },
                BarAction::Freeze,
                look.frozen,
            ),
            (icons::PIN, t!("widgets.pin"), BarAction::Pin, look.pinned),
            (
                icons::SETTINGS,
                t!("widgets.settings"),
                BarAction::Settings,
                false,
            ),
        ];
        let mut x = rect.right() - 6.0 - 9.0;
        for (icon, tip, action, active) in buttons {
            let r = egui::Rect::from_center_size(pos2(x, y), vec2(18.0, 18.0));
            let response = ui
                .interact(r, ui.scope_id().with(("barre", tip)), Sense::click())
                .on_hover_text(tip)
                .on_hover_cursor(CursorIcon::PointingHand);
            let color = if active {
                theme::ACCENT_2
            } else if response.hovered() {
                theme::TEXT
            } else {
                theme::TEXT_DIM
            };
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                icon,
                FontId::proportional(theme::SIZE_S),
                color,
            );
            if response.clicked() {
                out.bar = Some(action);
            }
            x -= 20.0;
        }
    }

    // Coin en bas à droite : redimensionner.
    if look.resizable && hovered {
        let corner = egui::Rect::from_min_max(rect.max - vec2(14.0, 14.0), rect.max);
        for i in 1..=2 {
            let d = 4.0 * i as f32;
            painter.line_segment(
                [
                    pos2(rect.right() - 3.0 - d, rect.bottom() - 3.0),
                    pos2(rect.right() - 3.0, rect.bottom() - 3.0 - d),
                ],
                Stroke::new(1.0, theme::TEXT_DIM),
            );
        }
        if let Some(p) = ctx.pointer_hover_pos()
            && corner.contains(p)
        {
            ctx.set_cursor_icon(CursorIcon::ResizeNwSe);
            if ctx.input(|i| i.pointer.primary_pressed()) {
                ctx.send_viewport_cmd(ViewportCommand::BeginResize(ResizeDirection::SouthEast));
            }
        }
    }
    out
}

/// Choix faits dans le panneau des réglages.
struct PanelState {
    frozen: bool,
    exclude: bool,
    pinned: bool,
    settings_changed: bool,
    close: bool,
    /// « Supprimer » choisi dans les réglages : passer à la confirmation.
    ask_delete: bool,
    delete: bool,
}

/// Panneau à côté d'un widget : réglages, ou confirmation de suppression.
#[allow(clippy::too_many_arguments)]
fn draw_panel(
    ui: &mut egui::Ui,
    mode: Panel,
    name: &str,
    icon: &str,
    kind: &str,
    widget: &mut dyn Widget,
    cx: &mut WidgetCx,
    st: &mut PanelState,
) {
    let ctx = ui.ctx().clone();
    let rect = ui.max_rect();
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(theme::CARD_RADIUS), theme::BG);
    painter.rect_stroke(
        rect,
        CornerRadius::same(theme::CARD_RADIUS),
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );
    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        st.close = true;
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(16.0)));
    let ui = &mut child;
    // En-tête : nom du widget et fermeture.
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(icon)
                .size(theme::SIZE_L)
                .color(theme::ACCENT_2),
        );
        widgets::section_title(ui, name);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon_button(ui, icons::CLOSE, t!("common.close")).clicked() {
                st.close = true;
            }
        });
    });
    ui.add_space(8.0);
    match mode {
        Panel::ConfirmDelete => {
            ui.label(RichText::new(t!("widgets.delete_title")).size(theme::SIZE_L));
            ui.add_space(4.0);
            let body = if kind == "notes" {
                t!("widgets.delete_note_body")
            } else {
                t!("widgets.delete_body")
            };
            ui.add(egui::Label::new(widgets::dim(body)).wrap());
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if widgets::danger(ui, t!("common.delete")).clicked() {
                    st.delete = true;
                }
                if ui.button(t!("common.cancel")).clicked() {
                    st.close = true;
                }
            });
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                st.delete = true;
            }
        }
        Panel::Settings => {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    if widget.settings_ui(ui, cx) {
                        st.settings_changed = true;
                    }
                    widgets::row_separator(ui);
                    widgets::section_title(ui, t!("widgets.common"));
                    ui.add_space(4.0);
                    widgets::setting_row(
                        ui,
                        icons::PIN,
                        t!("widgets.pin"),
                        Some(t!("widgets.pin_hint")),
                        |ui| widgets::toggle(ui, &mut st.pinned),
                    );
                    widgets::setting_row(
                        ui,
                        icons::LOCK,
                        t!("widgets.freeze"),
                        Some(t!("widgets.freeze_hint")),
                        |ui| widgets::toggle(ui, &mut st.frozen),
                    );
                    widgets::setting_row(
                        ui,
                        icons::EYE,
                        t!("widgets.exclude_hide"),
                        Some(t!("widgets.exclude_hide_hint")),
                        |ui| widgets::toggle(ui, &mut st.exclude),
                    );
                    ui.add_space(12.0);
                    if widgets::danger(ui, format!("{}  {}", icons::DELETE, t!("widgets.delete")))
                        .clicked()
                    {
                        // Confirmation dans le même panneau.
                        st.ask_delete = true;
                    }
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_spots_avoid_what_is_already_there() {
        let area = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let first = free_spot(area, &[], [200.0, 100.0]);
        assert_eq!((first.x, first.y), (40.0, 40.0));
        let second = free_spot(area, &[first], [200.0, 100.0]);
        assert!(!second.intersects(&first));
        assert_eq!(second.x, 40.0, "sous le premier, même colonne");
        // Écran plein : en cascade, dans l'écran.
        let full = [Rect::new(0.0, 0.0, 1000.0, 800.0)];
        let r = free_spot(area, &full, [200.0, 100.0]);
        assert!(area.contains(Point { x: r.x, y: r.y }));
    }

    #[test]
    fn panel_goes_right_then_left() {
        let screen = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let p = panel_position(
            Rect::new(100.0, 100.0, 200.0, 100.0),
            [300.0, 400.0],
            screen,
        );
        assert_eq!((p.x, p.y), (308.0, 100.0));
        let p = panel_position(
            Rect::new(700.0, 600.0, 200.0, 100.0),
            [300.0, 400.0],
            screen,
        );
        assert_eq!((p.x, p.y), (392.0, 400.0));
    }

    /// Cadre et panneau (réglages, confirmation) de chaque widget se dessinent sans
    /// écran.
    #[test]
    fn frames_and_panels_draw() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let look = FrameLook {
            frozen: true,
            pinned: true,
            bar_always: true,
            resizable: true,
        };
        for k in tidy_widgets::kinds() {
            let mut w = (k.create)(&Value::Null);
            let mut cx = WidgetCx::new(dir.path().to_path_buf());
            for mode in [Panel::Settings, Panel::ConfirmDelete] {
                let mut st = PanelState {
                    frozen: false,
                    exclude: false,
                    pinned: false,
                    settings_changed: false,
                    close: false,
                    ask_delete: false,
                    delete: false,
                };
                let out = ctx.run_ui(egui::RawInput::default(), |ui| {
                    let res = draw_frame(ui, w.as_mut(), &mut cx, &look);
                    assert!(res.bar.is_none(), "aucun clic");
                    draw_panel(
                        ui,
                        mode,
                        "essai",
                        k.icon,
                        k.id,
                        w.as_mut(),
                        &mut cx,
                        &mut st,
                    );
                });
                out.drop_without_applying_deltas();
                assert!(!st.delete && !st.close);
            }
        }
    }

    #[test]
    fn pinning_is_kept_in_the_settings() {
        let kind = tidy_widgets::kind("clock").unwrap();
        let w = (kind.create)(&Value::Null);
        let on = merged_settings(w.as_ref(), true);
        assert!(pinned_in(&on));
        let again = (kind.create)(&on);
        assert!(!pinned_in(&merged_settings(again.as_ref(), false)));
        assert_eq!(frame_size([10.0, 10.0]), MIN_SIZE);
    }
}
