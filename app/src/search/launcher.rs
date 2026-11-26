//! Fenêtre du lanceur : champ de saisie, onglets, liste des résultats, actions.
//!
//! Clavier : ↑↓ (et Page préc./suiv.) choisissent, Entrée ouvre, Ctrl+Entrée (⌘ sous
//! macOS) affiche dans l'Explorateur ou le Finder, Ctrl+Maj+C copie le chemin, Tab et
//! Maj+Tab changent d'onglet, Échap ferme. Clic droit : menu de l'Explorateur (Windows)
//! ou menu de Tidy. La fenêtre se cache dès qu'elle perd le focus.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use egui::{Align, Key, Layout, Modifiers, RichText, Sense, Stroke, UiBuilder};
use tidy_core::Store;
use tidy_core::category::{Category, classify_by_extension};
use tidy_core::events::Event;
use tidy_core::settings::Settings;
use tidy_search::{Config, Engine, Hit, Query, Recents, Tab};
use tidy_ui::{icons, t, theme, widgets};

use super::SIZE;
use super::icons::IconCache;
use crate::ipc::{self, Request};

/// Titre de la fenêtre (le menu de l'Explorateur la retrouve par son titre).
pub const TITLE: &str = "Tidy · Recherche";
/// Nombre maximal de résultats affichés.
const LIMIT: usize = 200;
/// Résultats de Spotlight demandés.
const MORE_LIMIT: usize = 100;
const ROW_HEIGHT: f32 = 46.0;
const ICON_SIZE: f32 = 28.0;
/// Durée d'affichage d'un message.
const NOTICE_FOR: Duration = Duration::from_secs(3);
/// Délai après « montrer » avant de suivre le focus.
const FOCUS_GRACE: Duration = Duration::from_millis(400);
/// Caché depuis ce délai, le lanceur s'arrête et rend sa mémoire (la fenêtre seule
/// coûte plusieurs dizaines de Mo) ; il redémarre depuis le cache à l'ouverture suivante.
const IDLE_EXIT: Duration = Duration::from_secs(15 * 60);

/// « Plus de résultats » (Spotlight).
enum More {
    Idle,
    Running {
        text: String,
        rx: Receiver<Vec<PathBuf>>,
    },
    Done {
        text: String,
        hits: Vec<Hit>,
    },
}

/// Action sur un résultat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowAction {
    Open,
    Reveal,
    CopyPath,
    Trash,
}

pub struct Launcher {
    store: Store,
    settings: Settings,
    config: Config,
    engine: Engine,
    recents: Recents,
    /// Change quand les récents changent (la requête est à refaire).
    recents_version: u64,
    events: Receiver<Event>,
    /// Rester en mémoire, fenêtre cachée, au lieu de se fermer.
    resident: bool,
    visible: bool,
    text: String,
    tab: Tab,
    hits: Vec<Hit>,
    /// Dernière requête soumise : texte, onglet, génération de l'index, récents.
    submitted: Option<(String, Tab, u64, u64)>,
    took: Option<Duration>,
    selected: usize,
    scroll_to_selected: bool,
    scroll_offset: f32,
    focus_field: bool,
    icons: IconCache,
    confirm_trash: Option<PathBuf>,
    more: More,
    notice: Option<(String, bool, Instant)>,
    /// La fenêtre a eu le focus depuis qu'elle est montrée.
    was_focused: bool,
    /// Caché depuis cet instant.
    hidden_at: Option<Instant>,
    /// Montrée à cet instant.
    shown_at: Instant,
    /// Le menu natif n'est pas disponible ici : menu de Tidy.
    own_menu: bool,
    home: Option<PathBuf>,
}

impl Launcher {
    pub fn new(
        store: Store,
        settings: Settings,
        engine: Engine,
        events: Receiver<Event>,
        resident: bool,
        query: Option<String>,
        ctx: egui::Context,
    ) -> Self {
        let recents = store.load();
        let config = Config::from_settings(&settings.search);
        if cfg!(target_os = "macos") && !cfg!(test) {
            tidy_platform::activate_process(std::process::id());
        }
        Self {
            store,
            settings,
            config,
            engine,
            recents,
            recents_version: 0,
            events,
            resident,
            visible: true,
            text: query.unwrap_or_default(),
            tab: Tab::All,
            hits: Vec::new(),
            submitted: None,
            took: None,
            selected: 0,
            scroll_to_selected: false,
            scroll_offset: 0.0,
            focus_field: true,
            icons: IconCache::new(ctx),
            confirm_trash: None,
            more: More::Idle,
            notice: None,
            was_focused: false,
            hidden_at: None,
            shown_at: Instant::now(),
            own_menu: false,
            home: tidy_search::home_dir(),
        }
    }

    // --- Montrer, cacher ---

    fn show(&mut self, ctx: &egui::Context, query: Option<&str>) {
        self.text = query.unwrap_or_default().to_owned();
        self.tab = Tab::All;
        self.reset_results();
        self.store.reload(&mut self.recents);
        self.recents_version += 1;
        let platform = tidy_platform::current();
        let pos = super::placement(&platform.screens(), platform.mouse_location(), SIZE);
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            pos[0], pos[1],
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        // macOS : une appli sans Dock ne passe pas devant d'elle-même.
        if cfg!(target_os = "macos") && !cfg!(test) {
            tidy_platform::activate_process(std::process::id());
        }
        self.shown_at = Instant::now();
        self.visible = true;
        self.hidden_at = None;
        self.was_focused = false;
        self.focus_field = true;
        ctx.request_repaint();
    }

    /// Cache la fenêtre (en mémoire) ou ferme le lanceur.
    fn hide(&mut self, ctx: &egui::Context) {
        if !self.resident {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        self.visible = false;
        self.text.clear();
        self.reset_results();
        self.confirm_trash = None;
        self.icons.clear();
        self.engine.save_cache();
        self.hidden_at = Some(Instant::now());
        ctx.request_repaint_after(IDLE_EXIT);
    }

    fn reset_results(&mut self) {
        self.hits.clear();
        self.submitted = None;
        self.took = None;
        self.selected = 0;
        self.scroll_offset = 0.0;
        self.more = More::Idle;
        self.notice = None;
    }

    // --- Événements de l'agent ---

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Module {
                    module,
                    action,
                    data,
                } if module == "search" => match action.as_str() {
                    "show" => {
                        let query = data.get("query").and_then(|q| q.as_str());
                        let focused = ctx.input(|i| i.viewport().focused) == Some(true);
                        // Le raccourci referme un lanceur déjà ouvert.
                        if self.visible && focused && query.is_none() {
                            self.hide(ctx);
                        } else {
                            self.show(ctx, query);
                        }
                    }
                    "reindex" => self.engine.rebuild(),
                    "recents_cleared" => {
                        self.store.reload(&mut self.recents);
                        self.recents_version += 1;
                    }
                    _ => {}
                },
                Event::SettingsChanged => self.reload_settings(),
                _ => {}
            }
        }
    }

    fn reload_settings(&mut self) {
        self.store.reload(&mut self.settings);
        crate::lang::apply(self.settings.language);
        let config = Config::from_settings(&self.settings.search);
        if config != self.config {
            self.engine.reconfigure(config.clone());
            self.config = config;
        }
        if !self.settings.search.keep_in_background {
            self.resident = false;
        }
        if !self.visible_tabs().contains(&self.tab) {
            self.tab = Tab::All;
        }
        self.submitted = None;
    }

    fn visible_tabs(&self) -> Vec<Tab> {
        Tab::visible(&self.settings.search.hidden_tabs)
    }

    // --- Requêtes ---

    /// Soumet la requête si la saisie, l'onglet, l'index ou les récents ont changé ;
    /// prend les résultats arrivés.
    fn update_results(&mut self) {
        let status = self.engine.status();
        let key = (
            self.text.clone(),
            self.tab,
            status.generation,
            self.recents_version,
        );
        if self.submitted.as_ref() != Some(&key) {
            let empty = self.text.trim().is_empty();
            let recents = if empty && !self.settings.search.show_recents {
                Vec::new()
            } else {
                self.recents.paths()
            };
            self.engine.submit(Query {
                text: self.text.clone(),
                tab: self.tab,
                limit: LIMIT,
                recents,
            });
            self.submitted = Some(key);
        }
        if let Some(results) = self.engine.take_results()
            && results.query.text == self.text
            && results.query.tab == self.tab
        {
            self.hits = results.hits;
            self.took = Some(results.took);
            self.selected = self.selected.min(self.row_count().saturating_sub(1));
        }
        if let More::Running { text, rx } = &self.more
            && let Ok(paths) = rx.try_recv()
        {
            let hits = paths
                .into_iter()
                .filter(|p| !self.hits.iter().any(|h| &h.path == p))
                .filter_map(|p| spotlight_hit(&p))
                .collect();
            self.more = More::Done {
                text: text.clone(),
                hits,
            };
        }
        // Saisie changée : les résultats de Spotlight ne valent plus.
        if matches!(&self.more, More::Running { text, .. } | More::Done { text, .. } if *text != self.text)
        {
            self.more = More::Idle;
        }
    }

    fn more_hits(&self) -> &[Hit] {
        match &self.more {
            More::Done { hits, .. } => hits,
            _ => &[],
        }
    }

    fn row_count(&self) -> usize {
        self.hits.len() + self.more_hits().len()
    }

    fn row(&self, i: usize) -> Option<&Hit> {
        self.hits
            .get(i)
            .or_else(|| self.more_hits().get(i.checked_sub(self.hits.len())?))
    }

    fn ask_more(&mut self, ctx: &egui::Context) {
        let text = self.text.trim().to_owned();
        let (tx, rx) = mpsc::channel();
        let wake = ctx.clone();
        let query = text.clone();
        let spawned = std::thread::Builder::new()
            .name("tidy-spotlight".into())
            .spawn(move || {
                let _ = tx.send(tidy_search::spotlight::more_results(&query, MORE_LIMIT));
                wake.request_repaint();
            });
        if spawned.is_ok() {
            self.more = More::Running {
                text: self.text.clone(),
                rx,
            };
        }
    }

    // --- Actions ---

    fn act(&mut self, ctx: &egui::Context, action: RowAction, index: usize) {
        let Some(hit) = self.row(index).cloned() else {
            return;
        };
        let platform = tidy_platform::current();
        match action {
            RowAction::Open => match platform.open(&hit.path) {
                Ok(()) => {
                    self.remember(&hit.path);
                    self.hide(ctx);
                }
                Err(e) => self.notify(t!("search.open_failed", error = format!("{e:#}")), true),
            },
            RowAction::Reveal => match platform.reveal(&hit.path) {
                Ok(()) => self.hide(ctx),
                Err(e) => self.notify(t!("search.open_failed", error = format!("{e:#}")), true),
            },
            RowAction::CopyPath => {
                ctx.copy_text(hit.path.display().to_string());
                self.notify(t!("search.path_copied").to_owned(), false);
            }
            RowAction::Trash => {
                if self.is_protected(&hit.path) {
                    self.notify(t!("search.trash_refused").to_owned(), true);
                } else {
                    self.confirm_trash = Some(hit.path);
                }
            }
        }
    }

    /// Un dossier indexé (ou un de ses parents) ne part jamais à la corbeille d'ici.
    fn is_protected(&self, path: &Path) -> bool {
        path.parent().is_none_or(|p| p.parent().is_none())
            || self.config.roots.iter().any(|r| r.path.starts_with(path))
            || self.home.as_deref().is_some_and(|h| h.starts_with(path))
    }

    fn trash(&mut self, path: &Path) {
        match tidy_platform::current().trash(&[path.to_path_buf()]) {
            Ok(()) => {
                self.recents.remove(path);
                self.save_recents();
                self.hits.retain(|h| h.path != path);
                if let More::Done { hits, .. } = &mut self.more {
                    hits.retain(|h| h.path != path);
                }
                self.selected = self.selected.min(self.row_count().saturating_sub(1));
                self.notify(t!("search.trash_done").to_owned(), false);
            }
            Err(e) => self.notify(format!("{e:#}"), true),
        }
    }

    fn remember(&mut self, path: &Path) {
        // Relue avant d'être réécrite : une lecture impossible ne doit pas remplacer
        // la liste par une liste vide.
        if !self.store.reload(&mut self.recents) {
            return;
        }
        self.recents.touch(path, tidy_search::now());
        self.recents_version += 1;
        self.save_recents();
    }

    fn save_recents(&self) {
        if let Err(e) = self.store.save(&self.recents) {
            log::warn!("récents non enregistrés : {e:#}");
        }
    }

    fn notify(&mut self, text: String, error: bool) {
        self.notice = Some((text, error, Instant::now()));
    }

    // --- Clavier ---

    fn keyboard(&mut self, ctx: &egui::Context) {
        if self.confirm_trash.is_some() {
            return;
        }
        let (up, down, page_up, page_down) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::PageUp),
                i.consume_key(Modifiers::NONE, Key::PageDown),
            )
        });
        let (reveal, open, copy, prev_tab, next_tab, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::C),
                i.consume_key(Modifiers::SHIFT, Key::Tab),
                i.consume_key(Modifiers::NONE, Key::Tab),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        let count = self.row_count();
        let page = 8;
        let before = self.selected;
        if count > 0 {
            if up {
                self.selected = self.selected.saturating_sub(1);
            }
            if down {
                self.selected = (self.selected + 1).min(count - 1);
            }
            if page_up {
                self.selected = self.selected.saturating_sub(page);
            }
            if page_down {
                self.selected = (self.selected + page).min(count - 1);
            }
        }
        if self.selected != before {
            self.scroll_to_selected = true;
        }
        if prev_tab || next_tab {
            let tabs = self.visible_tabs();
            let at = tabs.iter().position(|t| *t == self.tab).unwrap_or(0);
            let next = if next_tab {
                (at + 1) % tabs.len()
            } else {
                (at + tabs.len() - 1) % tabs.len()
            };
            self.set_tab(tabs[next]);
        }
        if reveal {
            self.act(ctx, RowAction::Reveal, self.selected);
        } else if open {
            self.act(ctx, RowAction::Open, self.selected);
        }
        if copy {
            self.act(ctx, RowAction::CopyPath, self.selected);
        }
        if escape {
            self.hide(ctx);
        }
    }

    fn set_tab(&mut self, tab: Tab) {
        if self.tab != tab {
            self.tab = tab;
            self.selected = 0;
            self.scroll_offset = 0.0;
        }
        self.focus_field = true;
    }

    // --- Dessin ---

    pub fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.icons.receive(&ctx);
        self.update_results();
        self.keyboard(&ctx);
        if !self.visible {
            return;
        }
        if self
            .notice
            .as_ref()
            .is_some_and(|(_, _, at)| at.elapsed() > NOTICE_FOR)
        {
            self.notice = None;
        }
        if self.notice.is_some() {
            ctx.request_repaint_after(NOTICE_FOR);
        }

        self.search_field(ui);
        ui.add_space(10.0);
        self.tabs(ui);
        ui.add_space(6.0);
        self.status_line(ui, &ctx);
        ui.add_space(4.0);
        let footer = 22.0;
        let list_height = (ui.available_height() - footer).max(ROW_HEIGHT);
        ui.allocate_ui(egui::vec2(ui.available_width(), list_height), |ui| {
            ui.set_min_height(list_height);
            self.list(ui, &ctx);
        });
        self.footer(ui);
        self.confirmation(&ctx);
    }

    fn search_field(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(icons::SEARCH)
                    .size(22.0)
                    .color(theme::ACCENT_2),
            );
            let buttons = 64.0;
            let edit = egui::TextEdit::singleline(&mut self.text)
                .hint_text(t!("search.placeholder"))
                .frame(egui::Frame::NONE)
                .font(egui::FontId::proportional(20.0))
                .desired_width(ui.available_width() - buttons);
            let response = ui.add(edit);
            if response.changed() {
                self.selected = 0;
                self.scroll_offset = 0.0;
            }
            let modal = self.confirm_trash.is_some();
            if !modal && (self.focus_field || !response.has_focus()) {
                response.request_focus();
                self.focus_field = false;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::icon_button(ui, icons::SETTINGS, t!("search.settings")).clicked() {
                    let _ = ipc::request(
                        &self.store,
                        &Request::OpenManager {
                            page: Some("search".into()),
                        },
                    );
                    let ctx = ui.ctx().clone();
                    self.hide(&ctx);
                }
                if !self.text.is_empty()
                    && widgets::icon_button(ui, icons::CLOSE, t!("search.clear")).clicked()
                {
                    self.text.clear();
                    self.selected = 0;
                    self.focus_field = true;
                }
            });
        });
    }

    fn tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for tab in self.visible_tabs() {
                if widgets::choice(ui, tab == self.tab, tab_label(tab)).clicked() {
                    self.set_tab(tab);
                }
            }
        });
    }

    fn status_line(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let status = self.engine.status();
        ui.horizontal(|ui| {
            let text = if let Some((text, error, _)) = &self.notice {
                let color = if *error {
                    theme::DANGER
                } else {
                    theme::ACCENT_2
                };
                RichText::new(text).size(theme::SIZE_S).color(color)
            } else if !status.ready {
                widgets::dim(t!("search.indexing_first"))
            } else if self.text.trim().is_empty() {
                widgets::dim(if self.hits.is_empty() {
                    t!("search.empty_hint", count = status.entries)
                } else {
                    t!("search.recents").to_owned()
                })
            } else {
                let ms = format!("{:.1}", self.took.map_or(0.0, |d| d.as_secs_f64() * 1000.0));
                widgets::dim(match self.hits.len() {
                    0 => t!("search.no_results").to_owned(),
                    1 => t!("search.count_one", ms = ms),
                    n => t!("search.count", count = n, ms = ms),
                })
            };
            ui.label(text);
            if status.scanning && status.ready {
                ui.label(widgets::dim(format!("· {}", t!("search.indexing"))));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let can_more = tidy_search::spotlight::available()
                    && self.settings.search.more_results
                    && !self.text.trim().is_empty();
                if !can_more {
                    return;
                }
                match &self.more {
                    More::Idle => {
                        if ui
                            .small_button(t!("search.more_results"))
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            self.ask_more(ctx);
                            self.focus_field = true;
                        }
                    }
                    More::Running { .. } => {
                        ui.label(widgets::dim(t!("search.more_running")));
                    }
                    More::Done { .. } => {}
                }
            });
        });
    }

    fn list(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let count = self.row_count();
        if count == 0 {
            return;
        }
        let view = ui.available_height();
        if self.scroll_to_selected {
            let top = self.selected as f32 * ROW_HEIGHT;
            if top < self.scroll_offset {
                self.scroll_offset = top;
            } else if top + ROW_HEIGHT > self.scroll_offset + view {
                self.scroll_offset = top + ROW_HEIGHT - view;
            }
        }
        let mut scroll = egui::ScrollArea::vertical()
            .auto_shrink(false)
            .id_salt("résultats");
        if self.scroll_to_selected {
            scroll = scroll.vertical_scroll_offset(self.scroll_offset);
            self.scroll_to_selected = false;
        }
        let px = (ICON_SIZE * ctx.pixels_per_point()).round() as u32;
        let mut chosen: Option<(RowAction, usize)> = None;
        let output = scroll.show_rows(ui, ROW_HEIGHT, count, |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for i in range {
                if let Some(action) = self.row_ui(ui, i, px) {
                    chosen = Some((action, i));
                }
            }
        });
        self.scroll_offset = output.state.clamped_offset().y;
        if let Some((action, i)) = chosen {
            self.selected = i;
            self.act(ctx, action, i);
        }
    }

    /// Une ligne de résultat ; rend l'action demandée.
    fn row_ui(&mut self, ui: &mut egui::Ui, i: usize, px: u32) -> Option<RowAction> {
        let hit = self.row(i)?.clone();
        let spotlight = i >= self.hits.len();
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
        let selected = i == self.selected;
        if ui.is_rect_visible(rect) {
            let fill = if selected {
                theme::ACCENT.gamma_multiply(0.35)
            } else if response.hovered() {
                theme::SURFACE_HI
            } else {
                egui::Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, 10, fill);
            if selected {
                let bar = egui::Rect::from_min_size(
                    rect.left_top() + egui::vec2(0.0, 10.0),
                    egui::vec2(3.0, ROW_HEIGHT - 20.0),
                );
                ui.painter().rect_filled(bar, 2, theme::ACCENT_2);
            }
        }
        let mut action = None;
        if response.double_clicked() {
            action = Some(RowAction::Open);
        } else if response.clicked() {
            self.selected = i;
            self.focus_field = true;
        }
        if response.secondary_clicked() {
            self.selected = i;
            if !self.own_menu {
                match tidy_platform::current()
                    .show_file_menu(TITLE, std::slice::from_ref(&hit.path))
                {
                    Ok(true) => {}
                    Ok(false) | Err(_) => self.own_menu = true,
                }
            }
        }
        if self.own_menu {
            response.context_menu(|ui| {
                for (a, icon, label) in row_actions() {
                    if ui.button(format!("{icon}  {label}")).clicked() {
                        action = Some(a);
                        ui.close();
                    }
                }
            });
        }

        let inner = rect.shrink2(egui::vec2(12.0, 4.0));
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::left_to_right(Align::Center)),
        );
        let ui = &mut child;
        match self.icons.get(&hit.path, px) {
            Some(texture) => {
                ui.add(egui::Image::new((
                    texture,
                    egui::vec2(ICON_SIZE, ICON_SIZE),
                )));
            }
            None => {
                let (icon, color) = category_icon(hit.category);
                let (r, _) =
                    ui.allocate_exact_size(egui::vec2(ICON_SIZE, ICON_SIZE), Sense::hover());
                ui.painter().text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    icon,
                    egui::FontId::proportional(22.0),
                    color,
                );
            }
        }
        ui.add_space(10.0);
        let show_actions = selected || response.hovered();
        let actions_width = if show_actions { 96.0 } else { 0.0 };
        let text_width = (ui.available_width() - actions_width - 70.0).max(40.0);
        ui.vertical(|ui| {
            ui.set_width(text_width);
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.add(
                egui::Label::new(RichText::new(hit.display_name()).size(theme::SIZE_M))
                    .truncate()
                    .selectable(false),
            );
            let parent = hit.path.parent().map(|p| self.short_path(p));
            ui.add(
                egui::Label::new(widgets::dim(parent.unwrap_or_default()))
                    .truncate()
                    .selectable(false),
            );
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if show_actions {
                for (a, icon, label) in row_actions().into_iter().rev() {
                    if a == RowAction::Open {
                        continue;
                    }
                    if widgets::icon_button(ui, icon, label).clicked() {
                        action = Some(a);
                    }
                }
            }
            if spotlight {
                ui.label(widgets::dim(t!("search.spotlight_badge")));
            } else if hit.recent {
                ui.label(
                    RichText::new(t!("search.recent_badge"))
                        .size(theme::SIZE_XS)
                        .color(theme::ACCENT_2),
                );
            }
        });
        action
    }

    /// Chemin du dossier, avec `~` pour le dossier personnel.
    fn short_path(&self, dir: &Path) -> String {
        if let Some(home) = &self.home
            && let Ok(rest) = dir.strip_prefix(home)
        {
            let rest = rest.display().to_string();
            return if rest.is_empty() {
                "~".into()
            } else {
                format!("~{}{rest}", std::path::MAIN_SEPARATOR)
            };
        }
        dir.display().to_string()
    }

    fn footer(&self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        let modifier = if cfg!(target_os = "macos") {
            "⌘"
        } else {
            "Ctrl"
        };
        ui.add(
            egui::Label::new(widgets::dim(t!("search.hint_keys", m = modifier)))
                .truncate()
                .selectable(false),
        );
    }

    fn confirmation(&mut self, ctx: &egui::Context) {
        let Some(path) = self.confirm_trash.clone() else {
            return;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match widgets::confirm(
            ctx,
            "corbeille-recherche",
            &t!("search.trash_title", name = name),
            t!("search.trash_body"),
            t!("search.trash"),
            true,
        ) {
            Some(true) => {
                self.confirm_trash = None;
                self.trash(&path);
                self.focus_field = true;
            }
            Some(false) => {
                self.confirm_trash = None;
                self.focus_field = true;
            }
            None => {}
        }
    }

    /// Cache la fenêtre quand elle perd le focus (après l'avoir eu).
    fn follow_focus(&mut self, ctx: &egui::Context) {
        if !self.visible {
            return;
        }
        // Juste après « montrer », l'état du focus peut dater d'avant : on l'ignore.
        let settled = self.shown_at.elapsed() >= FOCUS_GRACE;
        if !settled {
            ctx.request_repaint_after(FOCUS_GRACE);
        }
        match ctx.input(|i| i.viewport().focused) {
            Some(true) if settled => self.was_focused = true,
            Some(false) if self.was_focused => self.hide(ctx),
            _ => {}
        }
        // Fermeture demandée par le système (Alt+F4) : on cache seulement.
        if self.resident && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide(ctx);
        }
    }
}

impl eframe::App for Launcher {
    /// Tourne même fenêtre cachée : c'est là qu'arrive « montrer ».
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_events(ctx);
        self.follow_focus(ctx);
        if let Some(at) = self.hidden_at {
            if at.elapsed() >= IDLE_EXIT {
                log::info!("lanceur caché depuis {IDLE_EXIT:?} : arrêt");
                std::process::exit(0);
            }
            ctx.request_repaint_after(IDLE_EXIT.saturating_sub(at.elapsed()));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .corner_radius(theme::CARD_RADIUS)
                    .inner_margin(16),
            )
            .show(ui, |ui| self.draw(ui));
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }
}

fn row_actions() -> [(RowAction, &'static str, &'static str); 4] {
    let reveal = if cfg!(target_os = "macos") {
        t!("search.reveal_finder")
    } else {
        t!("search.reveal_explorer")
    };
    [
        (RowAction::Open, icons::OPEN, t!("search.open")),
        (RowAction::Reveal, icons::FOLDER_OPEN, reveal),
        (RowAction::CopyPath, icons::COPY, t!("search.copy_path")),
        (RowAction::Trash, icons::DELETE, t!("search.trash")),
    ]
}

/// Nom affiché d'un onglet.
pub fn tab_label(tab: Tab) -> &'static str {
    match tab {
        Tab::All => t!("search.tab_all"),
        Tab::Desktop => t!("search.tab_desktop"),
        Tab::Apps => t!("search.tab_apps"),
        Tab::Documents => t!("search.tab_documents"),
        Tab::Images => t!("search.tab_images"),
        Tab::Videos => t!("search.tab_videos"),
        Tab::Music => t!("search.tab_music"),
        Tab::Folders => t!("search.tab_folders"),
        Tab::Archives => t!("search.tab_archives"),
    }
}

/// Icône de repli d'une catégorie, avec sa couleur.
fn category_icon(category: Category) -> (&'static str, egui::Color32) {
    match category {
        Category::Application => (icons::APP, theme::ACCENT_2),
        Category::Folder => (icons::FOLDER, theme::ACCENT),
        Category::Document => (icons::DOCUMENT, theme::TEXT),
        Category::Image => (icons::IMAGE, theme::ACCENT),
        Category::Video => (icons::VIDEO, theme::ACCENT),
        Category::Music => (icons::MUSIC, theme::ACCENT),
        Category::Archive => (icons::ARCHIVE, theme::TEXT_DIM),
        Category::Executable => (icons::EXECUTABLE, theme::TEXT_DIM),
        Category::WebLink => (icons::LINK, theme::ACCENT),
        Category::Game => (icons::GAME, theme::ACCENT_2),
        Category::Other => (icons::FILE, theme::TEXT_DIM),
    }
}

/// Résultat de Spotlight (hors de l'index).
fn spotlight_hit(path: &Path) -> Option<Hit> {
    let name = path.file_name()?.to_str()?.to_owned();
    let meta = std::fs::metadata(path).ok()?;
    Some(Hit {
        path: path.to_path_buf(),
        category: classify_by_extension(&name, meta.is_dir()),
        is_dir: meta.is_dir(),
        name,
        modified: 0,
        recent: false,
        score: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le lanceur se dessine (saisie, onglets, résultats, confirmation) sans écran.
    #[test]
    fn draws_and_finds() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("racine");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("facture mars.pdf"), "x").unwrap();
        std::fs::write(root.join("photo été.jpg"), "x").unwrap();
        let store = Store::open(&dir.path().join("données")).unwrap();
        let mut settings = Settings::default();
        settings.search.disabled_defaults = tidy_search::DEFAULT_KEYS
            .iter()
            .map(|k| k.to_string())
            .collect();
        settings.search.folders = vec![root.clone()];
        let engine = Engine::start(Config::from_settings(&settings.search), None, || {});
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let (_tx, rx) = mpsc::channel();
        let mut l = Launcher::new(
            store,
            settings,
            engine,
            rx,
            true,
            Some("ete".into()),
            ctx.clone(),
        );
        let frame = |l: &mut Launcher| {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(SIZE[0], SIZE[1]),
                )),
                ..Default::default()
            };
            ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| l.draw(ui));
            })
            .drop_without_applying_deltas();
        };
        let start = Instant::now();
        while l.hits.is_empty() {
            assert!(start.elapsed() < Duration::from_secs(10), "pas de résultat");
            frame(&mut l);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(l.hits[0].name, "photo été.jpg");
        for tab in Tab::ALL {
            l.set_tab(tab);
            frame(&mut l);
        }
        l.confirm_trash = Some(root.join("facture mars.pdf"));
        frame(&mut l);
        assert!(
            l.confirm_trash.is_some(),
            "la confirmation attend une réponse"
        );
        assert!(l.is_protected(&root), "un dossier indexé n'est jamais jeté");
        assert!(!l.is_protected(&root.join("facture mars.pdf")));
    }
}
