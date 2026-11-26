//! Page « Fonds d'écran » (lot J) : bibliothèque en grille de miniatures, catégories,
//! favoris, application à un écran ou à tous, ajustement, couleur unie, playlist,
//! son, pause intelligente, sources en ligne et « Rendre le fond d'origine ».
//!
//! La page écrit la bibliothèque (`wallpapers.json`, seul écrivain) et les réglages
//! (`config.json`, via [`Manager::save`]) ; l'agent les applique. Les miniatures sont
//! faites sur un fil à part, une fois, puis lues dans le cache.
//!
//! L'état propre à la page (bibliothèque chargée, miniatures, sélection) vit dans la
//! mémoire d'egui : la structure [`Manager`] n'a pas à le connaître.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{Color32, Sense, Ui};
use serde::Deserialize;
use tidy_core::settings::wallpaper::{
    Choice, Fit, PLAYLIST_MAX_MINUTES, PLAYLIST_MIN_MINUTES, PlaylistOrder,
};
use tidy_ui::{icons, t, theme, widgets};
use tidy_wallpaper::library::{Filter, ImportReport};
use tidy_wallpaper::pause::PauseReason;
use tidy_wallpaper::thumbs::{self, Rgba};
use tidy_wallpaper::{IMAGE_EXTENSIONS, Library, MediaKind, VIDEO_EXTENSIONS, Wallpaper};
use uuid::Uuid;

use super::Manager;
use super::pages::{card_title, page_header};
use crate::ipc::{self, Request, Response};

/// Taille d'une miniature dans la grille (16:9).
const CELL: egui::Vec2 = egui::vec2(160.0, 90.0);
/// Hauteur du nom sous la miniature.
const LABEL_HEIGHT: f32 = 20.0;
/// Fréquence de mise à jour de l'état des fonds (agent).
const STATUS_EVERY: Duration = Duration::from_secs(3);
/// Miniatures gardées en mémoire graphique au plus (les moins récemment vues partent ;
/// elles restent en cache sur le disque et reviennent vite).
const MAX_TEXTURES: usize = 96;

/// État des fonds rendu par l'agent (`status`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct AgentStatus {
    video_running: bool,
    gave_up: bool,
    paused: Option<PauseReason>,
    original_saved: bool,
    original_missing: bool,
    next_in: Option<u64>,
    error: Option<String>,
}

/// Fil des miniatures : reçoit des fonds, rend leur miniature décodée.
struct ThumbWorker {
    jobs: Sender<Wallpaper>,
    results: Receiver<(Uuid, Option<Rgba>)>,
}

impl ThumbWorker {
    fn spawn(root: PathBuf, ctx: egui::Context) -> Option<Self> {
        let (jobs, job_rx) = mpsc::channel::<Wallpaper>();
        let (result_tx, results) = mpsc::channel();
        std::thread::Builder::new()
            .name("tidy-miniatures".into())
            .spawn(move || {
                for w in job_rx {
                    let image = make_thumb(&root, &w);
                    if result_tx.send((w.id, image)).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            })
            .inspect_err(|e| log::warn!("fil des miniatures : {e}"))
            .ok()?;
        Some(Self { jobs, results })
    }
}

/// Miniature d'un fond : depuis le cache, sinon faite (et l'image préparée pour le
/// système si besoin).
fn make_thumb(root: &Path, w: &Wallpaper) -> Option<Rgba> {
    let path = thumbs::thumb_path(root, w.id);
    if !path.is_file() {
        if !w.available() {
            return None;
        }
        let made = match w.kind {
            MediaKind::Image => {
                if let Err(e) = thumbs::prepare_system_image(root, w) {
                    log::warn!("conversion de {} : {e:#}", w.path.display());
                }
                thumbs::make_image_thumb(root, w).map(|_| ())
            }
            MediaKind::Video => match video_frame(&w.path) {
                Some(frame) => thumbs::save_thumb(root, w.id, frame).map(|_| ()),
                None => Err(anyhow::anyhow!("pas d'image pour cette vidéo")),
            },
        };
        if let Err(e) = made {
            log::debug!("miniature de {} : {e:#}", w.path.display());
            return None;
        }
    }
    thumbs::load(&path)
}

/// Image d'une vidéo : AVFoundation (macOS), sinon la vignette de l'Explorateur.
fn video_frame(path: &Path) -> Option<Rgba> {
    tidy_wallpaper::player::poster_frame(path, thumbs::THUMB_WIDTH).or_else(|| {
        let icon = tidy_platform::current().file_icon(path, 256, true)?;
        Some(Rgba {
            width: icon.width,
            height: icon.height,
            pixels: icon.rgba,
        })
    })
}

#[derive(Default)]
struct PageState {
    library: Library,
    loaded: bool,
    filter: Filter,
    selected: Option<Uuid>,
    /// Écran visé par « Appliquer » (`None` : tous les écrans).
    target: Option<String>,
    /// Miniatures chargées, avec l'image où elles ont servi pour la dernière fois.
    textures: HashMap<Uuid, (egui::TextureHandle, u64)>,
    /// Miniatures demandées (ou impossibles) : pas redemandées.
    requested: HashSet<Uuid>,
    worker: Option<ThumbWorker>,
    /// Numéro de l'image en cours (âge des miniatures).
    frame: u64,
    new_category: String,
    /// Couleur unie en cours de choix.
    color: Option<Color32>,
    status: Arc<Mutex<Option<AgentStatus>>>,
    status_busy: Arc<AtomicBool>,
    status_at: Option<Instant>,
}

type Shared = Arc<Mutex<PageState>>;

fn page_state(ctx: &egui::Context) -> Shared {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Shared>(egui::Id::unique("page-fonds-d-ecran"))
            .clone()
    })
}

/// Demande à l'agent, sur un fil à part (l'agent peut mettre un moment à répondre) ;
/// sans agent, rien ne se passe : il relira tout à son démarrage.
fn tell_agent(m: &Manager, action: &'static str) {
    let store = m.store.clone();
    std::thread::spawn(move || {
        let _ = ipc::request(&store, &module_request(action));
    });
}

fn module_request(action: &str) -> Request {
    Request::Module {
        module: "wallpaper".into(),
        action: action.into(),
        data: serde_json::Value::Null,
    }
}

impl PageState {
    fn load(&mut self, m: &Manager) {
        if !self.loaded {
            self.library = m.store.load();
            self.loaded = true;
        }
    }

    fn save_library(&mut self, m: &mut Manager) {
        match m.store.save(&self.library) {
            Ok(()) => tell_agent(m, "reload"),
            Err(e) => m.error = Some(format!("{e:#}")),
        }
    }

    /// Relève l'état de l'agent de temps en temps, sans bloquer la fenêtre.
    fn refresh_status(&mut self, m: &Manager, ctx: &egui::Context) {
        if self.status_at.is_some_and(|t| t.elapsed() < STATUS_EVERY)
            || self.status_busy.swap(true, Ordering::AcqRel)
        {
            return;
        }
        self.status_at = Some(Instant::now());
        let (store, slot, busy, repaint) = (
            m.store.clone(),
            Arc::clone(&self.status),
            Arc::clone(&self.status_busy),
            ctx.clone(),
        );
        std::thread::spawn(move || {
            let status = match ipc::request(&store, &module_request("status")) {
                Ok(Response::Data { data }) => serde_json::from_value(data).ok(),
                _ => None,
            };
            if let Ok(mut slot) = slot.lock() {
                *slot = status;
            }
            busy.store(false, Ordering::Release);
            repaint.request_repaint();
        });
        ctx.request_repaint_after(STATUS_EVERY);
    }

    /// Miniatures prêtes. Elles ne sont demandées qu'à l'affichage de leur case
    /// ([`PageState::thumb`]).
    fn pump_thumbs(&mut self, m: &Manager, ctx: &egui::Context) {
        self.frame += 1;
        if self.worker.is_none() && !self.library.items.is_empty() {
            self.worker = ThumbWorker::spawn(m.store.root().to_owned(), ctx.clone());
        }
        if let Some(worker) = &self.worker {
            while let Ok((id, image)) = worker.results.try_recv() {
                if let Some(img) = image {
                    let color = egui::ColorImage::from_rgba_unmultiplied(
                        [img.width as usize, img.height as usize],
                        &img.pixels,
                    );
                    let texture =
                        ctx.load_texture(format!("fond-{id}"), color, egui::TextureOptions::LINEAR);
                    self.textures.insert(id, (texture, self.frame));
                }
            }
        }
    }

    /// Miniature d'une case visible : demandée si elle manque, notée comme vue.
    fn thumb(&mut self, w: &Wallpaper) -> Option<egui::TextureHandle> {
        if let Some((texture, seen)) = self.textures.get_mut(&w.id) {
            *seen = self.frame;
            return Some(texture.clone());
        }
        if let Some(worker) = &self.worker
            && self.requested.insert(w.id)
        {
            let _ = worker.jobs.send(w.clone());
        }
        None
    }

    /// Garde au plus [`MAX_TEXTURES`] miniatures : les moins récemment vues partent
    /// (redemandées si leur case revient à l'écran).
    fn trim_textures(&mut self) {
        let excess = self.textures.len().saturating_sub(MAX_TEXTURES);
        if excess == 0 {
            return;
        }
        let mut by_age: Vec<(u64, Uuid)> = self
            .textures
            .iter()
            .filter(|(_, (_, seen))| *seen < self.frame)
            .map(|(id, (_, seen))| (*seen, *id))
            .collect();
        by_age.sort_unstable();
        for (_, id) in by_age.into_iter().take(excess) {
            self.textures.remove(&id);
            self.requested.remove(&id);
        }
    }
}

pub fn show(m: &mut Manager, ui: &mut Ui) {
    let shared = page_state(ui.ctx());
    let mut st = shared.lock().unwrap_or_else(|p| p.into_inner());
    st.load(m);
    st.pump_thumbs(m, ui.ctx());
    st.refresh_status(m, ui.ctx());

    page_header(
        ui,
        icons::WALLPAPER,
        t!("manager.wallpapers"),
        t!("wallpapers.intro"),
    );
    overview(m, &mut st, ui);
    ui.add_space(12.0);
    placement(m, &mut st, ui);
    ui.add_space(12.0);
    library(m, &mut st, ui);
    ui.add_space(12.0);
    solid_color(m, &mut st, ui);
    ui.add_space(12.0);
    playlist(m, &mut st, ui);
    ui.add_space(12.0);
    sound(m, ui);
    ui.add_space(12.0);
    smart_pause(m, ui);
    ui.add_space(12.0);
    online(m, &mut st, ui);
}

/// Applique un choix à l'écran visé (ou à tous) et active les fonds de Tidy.
fn apply_choice(m: &mut Manager, st: &PageState, choice: Choice) {
    let w = &mut m.settings.wallpaper;
    w.set_choice(st.target.as_deref(), choice);
    w.enabled = true;
    m.save();
    m.show_notice(t!("wallpapers.applied"));
}

fn format_duration(minutes: u32) -> String {
    let (h, min) = (minutes / 60, minutes % 60);
    match (h, min) {
        (0, m) => t!("wallpapers.minutes", n = m),
        (h, 0) => t!("wallpapers.hours", n = h),
        (h, m) => t!("wallpapers.hours_minutes", h = h, m = format!("{m:02}")),
    }
}

/// Ce qu'affiche un choix, en clair.
fn describe(choice: &Choice, library: &Library) -> String {
    let named = |id: Uuid| match library.get(id) {
        Some(w) if w.kind == MediaKind::Video => t!("wallpapers.choice_video", name = w.name),
        Some(w) => t!("wallpapers.choice_image", name = w.name),
        None => t!("wallpapers.choice_missing").to_owned(),
    };
    match choice {
        Choice::None => t!("wallpapers.choice_none").to_owned(),
        Choice::Item { id } => named(*id),
        Choice::Color { .. } => t!("wallpapers.choice_color").to_owned(),
        Choice::Playlist => t!("wallpapers.choice_playlist").to_owned(),
        Choice::Online { .. } => t!("wallpapers.choice_online").to_owned(),
    }
}

/// Activation, état et « Rendre le fond d'origine ».
fn overview(m: &mut Manager, st: &mut PageState, ui: &mut Ui) {
    let status = st.status.lock().ok().and_then(|s| s.clone());
    widgets::card(ui, |ui| {
        card_title(ui, icons::WALLPAPER, t!("wallpapers.overview_title"));
        let mut enabled = m.settings.wallpaper.enabled;
        let changed = widgets::setting_row(
            ui,
            icons::POWER,
            t!("wallpapers.enabled"),
            Some(t!("wallpapers.enabled_hint")),
            |ui| widgets::toggle(ui, &mut enabled).changed(),
        );
        if changed {
            m.settings.wallpaper.enabled = enabled;
            m.save();
        }
        widgets::row_separator(ui);
        let state_text = match (&status, m.settings.wallpaper.enabled) {
            (None, _) => t!("wallpapers.status_agent_off").to_owned(),
            (Some(_), false) => t!("wallpapers.status_disabled").to_owned(),
            (Some(s), true) => match s.paused {
                Some(PauseReason::Fullscreen) => t!("wallpapers.status_paused_fullscreen").into(),
                Some(PauseReason::Battery) => t!("wallpapers.status_paused_battery").into(),
                Some(PauseReason::Locked) => t!("wallpapers.status_paused_locked").into(),
                Some(PauseReason::DesktopHidden) => t!("wallpapers.status_paused_hidden").into(),
                None if s.video_running => t!("wallpapers.status_playing").into(),
                None => t!("wallpapers.status_static").into(),
            },
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(widgets::dim(state_text));
            if let Some(secs) = status.as_ref().and_then(|s| s.next_in)
                && m.settings.wallpaper.enabled
            {
                let minutes = secs.div_ceil(60) as u32;
                ui.label(widgets::dim(t!(
                    "wallpapers.next_in",
                    time = format_duration(minutes.max(1))
                )));
            }
        });
        if let Some(s) = &status {
            if s.gave_up {
                ui.add_space(6.0);
                widgets::notice(
                    ui,
                    theme::WARNING,
                    icons::WARNING,
                    t!("wallpapers.status_gave_up"),
                );
            }
            if s.original_missing && m.settings.wallpaper.enabled {
                ui.add_space(6.0);
                widgets::notice(
                    ui,
                    theme::WARNING,
                    icons::WARNING,
                    t!("wallpapers.status_original_missing"),
                );
            }
            if let Some(error) = &s.error {
                ui.add_space(6.0);
                widgets::notice(ui, theme::DANGER, icons::WARNING, error);
            }
        }
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if m.settings.wallpaper.uses_playlist()
                && ui
                    .button(format!("{}  {}", icons::NEXT, t!("wallpapers.next")))
                    .clicked()
            {
                tell_agent(m, "next");
            }
            let can_restore =
                status.as_ref().is_some_and(|s| s.original_saved) || m.settings.wallpaper.enabled;
            if ui
                .add_enabled(
                    can_restore,
                    egui::Button::new(format!("{}  {}", icons::RESTORE, t!("wallpapers.restore"))),
                )
                .on_hover_text(t!("wallpapers.restore_hint"))
                .clicked()
            {
                m.settings.wallpaper.enabled = false;
                m.save();
                tell_agent(m, "restore_original");
                m.show_notice(t!("wallpapers.restored"));
            }
        });
    });
}

/// Écran visé, ce qu'il affiche, ajustement et couleur de fond.
fn placement(m: &mut Manager, st: &mut PageState, ui: &mut Ui) {
    let screens = tidy_platform::current().screens();
    if st
        .target
        .as_ref()
        .is_some_and(|t| !screens.iter().any(|s| s.id.0 == *t))
    {
        st.target = None;
    }
    widgets::card(ui, |ui| {
        card_title(ui, icons::MONITOR, t!("wallpapers.where_title"));
        ui.horizontal_wrapped(|ui| {
            if widgets::choice(ui, st.target.is_none(), t!("wallpapers.all_screens")).clicked() {
                st.target = None;
            }
            if screens.len() > 1 {
                for s in &screens {
                    let label = if s.primary {
                        format!("{} {}", s.name, icons::STAR)
                    } else {
                        s.name.clone()
                    };
                    if widgets::choice(ui, st.target.as_deref() == Some(&s.id.0), label).clicked() {
                        st.target = Some(s.id.0.clone());
                    }
                }
            }
        });
        let w = &m.settings.wallpaper;
        let current = match &st.target {
            None if !w.same_on_all => t!("wallpapers.per_screen").to_owned(),
            None => describe(&w.all, &st.library),
            Some(id) => describe(w.choice_for(id), &st.library),
        };
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(widgets::dim(t!("wallpapers.current", what = current)));
            if ui
                .small_button(t!("wallpapers.clear_screen"))
                .on_hover_text(t!("wallpapers.clear_screen_hint"))
                .clicked()
            {
                m.settings
                    .wallpaper
                    .set_choice(st.target.as_deref(), Choice::None);
                m.save();
            }
        });
        widgets::row_separator(ui);
        let mut fit = m.settings.wallpaper.fit;
        let changed = widgets::setting_row(
            ui,
            icons::CORNERS,
            t!("wallpapers.fit"),
            Some(t!("wallpapers.fit_hint")),
            |ui| {
                let mut changed = false;
                for (value, label) in [
                    (Fit::Fill, t!("wallpapers.fit_fill")),
                    (Fit::Fit, t!("wallpapers.fit_fit")),
                    (Fit::Stretch, t!("wallpapers.fit_stretch")),
                    (Fit::Center, t!("wallpapers.fit_center")),
                    (Fit::Tile, t!("wallpapers.fit_tile")),
                ]
                .into_iter()
                .rev()
                {
                    changed |= widgets::choice_value(ui, &mut fit, value, label).changed();
                }
                changed
            },
        );
        if changed {
            m.settings.wallpaper.fit = fit;
            m.save();
        }
        widgets::row_separator(ui);
        ui.label(t!("wallpapers.background"));
        let [r, g, b] = m.settings.wallpaper.background;
        let mut color = Color32::from_rgb(r, g, b);
        if widgets::swatches(ui, &mut color) {
            m.settings.wallpaper.background = [color.r(), color.g(), color.b()];
            m.save_later();
        }
    });
}

fn library(m: &mut Manager, st: &mut PageState, ui: &mut Ui) {
    widgets::card(ui, |ui| {
        card_title(ui, icons::IMAGE, t!("wallpapers.library_title"));
        widgets::hint(ui, t!("wallpapers.library_hint"));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if widgets::primary(
                ui,
                format!("{}  {}", icons::PLUS, t!("wallpapers.import_files")),
            )
            .clicked()
            {
                import_files(m, st);
            }
            if ui
                .button(format!(
                    "{}  {}",
                    icons::FOLDER_PLUS,
                    t!("wallpapers.import_folder")
                ))
                .clicked()
            {
                import_folder(m, st);
            }
        });
        ui.add_space(8.0);
        filters(m, st, ui);
        ui.add_space(8.0);
        grid(m, st, ui);
        if let Some(id) = st.selected
            && st.library.get(id).is_some()
        {
            widgets::row_separator(ui);
            details(m, st, ui, id);
        }
    });
}

fn report_import(m: &mut Manager, st: &mut PageState, report: &ImportReport) {
    if let Some(first) = report.added.first() {
        st.selected = Some(*first);
    }
    st.save_library(m);
    m.show_notice(t!(
        "wallpapers.imported",
        added = report.added.len(),
        duplicates = report.duplicates,
        unsupported = report.unsupported
    ));
}

fn import_files(m: &mut Manager, st: &mut PageState) {
    let extensions: Vec<&str> = IMAGE_EXTENSIONS
        .iter()
        .chain(VIDEO_EXTENSIONS)
        .copied()
        .collect();
    let Some(paths) = rfd::FileDialog::new()
        .set_title(t!("wallpapers.import_dialog"))
        .add_filter(t!("wallpapers.import_filter"), &extensions)
        .pick_files()
    else {
        return;
    };
    let category = match &st.filter {
        Filter::Category(c) => Some(c.clone()),
        _ => None,
    };
    let report = st.library.import_files(&paths, category.as_deref());
    report_import(m, st, &report);
}

fn import_folder(m: &mut Manager, st: &mut PageState) {
    let Some(dir) = rfd::FileDialog::new()
        .set_title(t!("wallpapers.import_folder_dialog"))
        .pick_folder()
    else {
        return;
    };
    let report = st.library.import_dir(&dir);
    report_import(m, st, &report);
}

fn filters(m: &mut Manager, st: &mut PageState, ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        let mut filter = st.filter.clone();
        widgets::choice_value(ui, &mut filter, Filter::All, t!("wallpapers.filter_all"));
        widgets::choice_value(
            ui,
            &mut filter,
            Filter::Favorites,
            format!("{} {}", icons::STAR, t!("wallpapers.filter_favorites")),
        );
        widgets::choice_value(
            ui,
            &mut filter,
            Filter::Images,
            format!("{} {}", icons::IMAGE, t!("wallpapers.filter_images")),
        );
        widgets::choice_value(
            ui,
            &mut filter,
            Filter::Videos,
            format!("{} {}", icons::VIDEO, t!("wallpapers.filter_videos")),
        );
        for c in st.library.categories.clone() {
            widgets::choice_value(ui, &mut filter, Filter::Category(c.clone()), c);
        }
        st.filter = filter;
    });
    ui.horizontal_wrapped(|ui| {
        let edit = egui::TextEdit::singleline(&mut st.new_category)
            .hint_text(t!("wallpapers.new_category"))
            .desired_width(180.0);
        let response = ui.add(edit);
        let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button(t!("wallpapers.add_category")).clicked() || enter)
            && let Some(name) = st.library.add_category(&st.new_category)
        {
            st.new_category.clear();
            st.filter = Filter::Category(name);
            st.save_library(m);
        }
        if let Filter::Category(c) = st.filter.clone()
            && ui
                .button(format!(
                    "{}  {}",
                    icons::DELETE,
                    t!("wallpapers.delete_category")
                ))
                .on_hover_text(t!("wallpapers.delete_category_hint"))
                .clicked()
        {
            st.library.remove_category(&c);
            st.filter = Filter::All;
            st.save_library(m);
        }
    });
}

fn grid(m: &mut Manager, st: &mut PageState, ui: &mut Ui) {
    if st.library.items.is_empty() {
        widgets::hint(ui, t!("wallpapers.empty"));
        return;
    }
    let items: Vec<Wallpaper> = st.library.filtered(&st.filter).cloned().collect();
    if items.is_empty() {
        widgets::hint(ui, t!("wallpapers.empty_filter"));
        return;
    }
    widgets::hint(ui, t!("wallpapers.apply_hint"));
    ui.add_space(4.0);
    let mut apply = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
        for w in &items {
            let response = cell(ui, st, w);
            if response.clicked() {
                st.selected = Some(w.id);
            }
            if response.double_clicked() {
                apply = Some(w.id);
            }
        }
    });
    st.trim_textures();
    if let Some(id) = apply {
        apply_item(m, st, id);
    }
}

/// Une miniature cliquable, avec son nom.
fn cell(ui: &mut Ui, st: &mut PageState, w: &Wallpaper) -> egui::Response {
    let size = egui::vec2(CELL.x, CELL.y + LABEL_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = response
        .on_hover_text(w.path.display().to_string())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let image_rect = egui::Rect::from_min_size(rect.min, CELL);
    // Seulement pour une case visible : pas de miniature pour toute la bibliothèque.
    let texture = st.thumb(w);
    let painter = ui.painter();
    painter.rect_filled(image_rect, 10, theme::SURFACE_HI);
    match texture {
        Some(texture) => {
            let [tw, th] = texture.size().map(|v| v as f32);
            // Remplir la case sans déformer : on coupe ce qui dépasse.
            let (cell_ratio, ratio) = (CELL.x / CELL.y, tw / th.max(1.0));
            let uv = if ratio > cell_ratio {
                let visible = cell_ratio / ratio;
                egui::Rect::from_min_max(
                    egui::pos2((1.0 - visible) / 2.0, 0.0),
                    egui::pos2((1.0 + visible) / 2.0, 1.0),
                )
            } else {
                let visible = ratio / cell_ratio;
                egui::Rect::from_min_max(
                    egui::pos2(0.0, (1.0 - visible) / 2.0),
                    egui::pos2(1.0, (1.0 + visible) / 2.0),
                )
            };
            egui::Image::new(egui::load::SizedTexture::new(texture.id(), CELL))
                .uv(uv)
                .corner_radius(10)
                .paint_at(ui, image_rect);
        }
        None => {
            let icon = if w.kind == MediaKind::Video {
                icons::VIDEO
            } else {
                icons::IMAGE
            };
            painter.text(
                image_rect.center(),
                egui::Align2::CENTER_CENTER,
                icon,
                egui::FontId::proportional(28.0),
                theme::TEXT_DIM,
            );
        }
    }
    let badge = |pos: egui::Pos2, align: egui::Align2, icon: &str, color: Color32| {
        let galley = painter.layout_no_wrap(
            icon.to_owned(),
            egui::FontId::proportional(theme::SIZE_M),
            color,
        );
        let r = align.anchor_size(pos, galley.size() + egui::vec2(8.0, 6.0));
        painter.rect_filled(r, 6, theme::BG.gamma_multiply(0.7));
        painter.galley(r.min + egui::vec2(4.0, 3.0), galley, color);
    };
    let inner = image_rect.shrink(6.0);
    if w.kind == MediaKind::Video {
        badge(
            inner.left_top(),
            egui::Align2::LEFT_TOP,
            icons::VIDEO,
            theme::TEXT,
        );
    }
    if w.favorite {
        badge(
            inner.right_top(),
            egui::Align2::RIGHT_TOP,
            icons::STAR,
            theme::ACCENT_2,
        );
    }
    if !w.available() {
        badge(
            inner.left_bottom(),
            egui::Align2::LEFT_BOTTOM,
            icons::WARNING,
            theme::DANGER,
        );
    }
    let selected = st.selected == Some(w.id);
    if selected {
        painter.rect_stroke(
            image_rect,
            10,
            egui::Stroke::new(2.0, theme::ACCENT_2),
            egui::StrokeKind::Outside,
        );
    } else if response.hovered() {
        painter.rect_stroke(
            image_rect,
            10,
            egui::Stroke::new(1.0, theme::TEXT_DIM),
            egui::StrokeKind::Outside,
        );
    }
    let name = widgets::truncate(&w.name, 24);
    painter.text(
        egui::pos2(rect.left() + 2.0, image_rect.bottom() + 4.0),
        egui::Align2::LEFT_TOP,
        name,
        egui::FontId::proportional(theme::SIZE_S),
        if selected {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        },
    );
    response
}

/// Applique un fond de la bibliothèque à l'écran visé.
fn apply_item(m: &mut Manager, st: &mut PageState, id: Uuid) {
    let Some(w) = st.library.get(id) else {
        return;
    };
    if !w.available() {
        m.error = Some(t!("wallpapers.missing").into());
        return;
    }
    // Image que le système ne sait pas poser (WebP) : convertie tout de suite.
    if let Err(e) = thumbs::prepare_system_image(m.store.root(), w) {
        m.error = Some(format!("{e:#}"));
        return;
    }
    apply_choice(m, st, Choice::Item { id });
}

/// Détail du fond sélectionné : appliquer, favori, catégorie, playlist, retirer.
fn details(m: &mut Manager, st: &mut PageState, ui: &mut Ui, id: Uuid) {
    let Some(w) = st.library.get(id).cloned() else {
        return;
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(&w.name).size(theme::SIZE_L));
        ui.label(widgets::dim(match w.kind {
            MediaKind::Image => t!("wallpapers.kind_image"),
            MediaKind::Video => t!("wallpapers.kind_video"),
        }));
    });
    let path = w.path.display().to_string();
    ui.add(egui::Label::new(widgets::dim(&path)).truncate())
        .on_hover_text(path);
    if !w.available() {
        ui.add_space(4.0);
        widgets::notice(ui, theme::WARNING, icons::WARNING, t!("wallpapers.missing"));
    }
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        if widgets::primary(ui, format!("{}  {}", icons::OK, t!("wallpapers.apply"))).clicked() {
            apply_item(m, st, id);
        }
        let star = if w.favorite {
            t!("wallpapers.unfavorite")
        } else {
            t!("wallpapers.favorite")
        };
        if ui.button(format!("{}  {star}", icons::STAR)).clicked() {
            st.library.set_favorite(id, !w.favorite);
            st.save_library(m);
        }
        let in_playlist = m.settings.wallpaper.playlist.items.contains(&id);
        let label = if in_playlist {
            t!("wallpapers.remove_from_playlist")
        } else {
            t!("wallpapers.add_to_playlist")
        };
        if ui.button(format!("{}  {label}", icons::LIST)).clicked() {
            let items = &mut m.settings.wallpaper.playlist.items;
            if in_playlist {
                items.retain(|x| *x != id);
            } else {
                items.push(id);
            }
            m.save();
        }
        if ui
            .button(format!(
                "{}  {}",
                icons::FOLDER_OPEN,
                t!("wallpapers.reveal")
            ))
            .clicked()
            && let Err(e) = tidy_platform::current().reveal(&w.path)
        {
            m.error = Some(format!("{e:#}"));
        }
        if ui
            .button(format!("{}  {}", icons::DELETE, t!("wallpapers.remove")))
            .on_hover_text(t!("wallpapers.remove_hint"))
            .clicked()
        {
            st.library.remove(id);
            thumbs::forget(m.store.root(), id);
            st.textures.remove(&id);
            st.requested.remove(&id);
            st.selected = None;
            st.save_library(m);
            m.settings.wallpaper.forget_item(id);
            m.save();
            m.show_notice(t!("wallpapers.removed"));
        }
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(t!("wallpapers.category"));
        let none = t!("wallpapers.no_category");
        let mut category = w.category.clone();
        egui::ComboBox::from_id_salt(("catégorie-fond", id))
            .selected_text(category.clone().unwrap_or_else(|| none.to_owned()))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut category, None, none);
                for c in &st.library.categories {
                    ui.selectable_value(&mut category, Some(c.clone()), c);
                }
            });
        if category != w.category {
            st.library.set_category(id, category.as_deref());
            st.save_library(m);
        }
    });
}

fn solid_color(m: &mut Manager, st: &mut PageState, ui: &mut Ui) {
    widgets::card(ui, |ui| {
        card_title(ui, icons::PALETTE, t!("wallpapers.color_title"));
        let mut color = st.color.unwrap_or_else(|| {
            match m
                .settings
                .wallpaper
                .choice_for(st.target.as_deref().unwrap_or(""))
            {
                Choice::Color { rgb: [r, g, b] } => Color32::from_rgb(*r, *g, *b),
                _ => Color32::from_rgb(0x10, 0x10, 0x12),
            }
        });
        if widgets::swatches(ui, &mut color) {
            st.color = Some(color);
        }
        ui.add_space(6.0);
        if widgets::primary(ui, t!("wallpapers.apply_color")).clicked() {
            apply_choice(
                m,
                st,
                Choice::Color {
                    rgb: [color.r(), color.g(), color.b()],
                },
            );
        }
    });
}

fn playlist(m: &mut Manager, st: &mut PageState, ui: &mut Ui) {
    widgets::card(ui, |ui| {
        card_title(ui, icons::LAYOUT_LIST, t!("wallpapers.playlist_title"));
        widgets::hint(ui, t!("wallpapers.playlist_hint"));
        ui.add_space(6.0);
        let items = m.settings.wallpaper.playlist.items.clone();
        if items.is_empty() {
            ui.label(widgets::dim(t!("wallpapers.playlist_empty")));
        }
        let mut change: Option<(usize, i32)> = None;
        for (i, id) in items.iter().enumerate() {
            ui.horizontal(|ui| {
                let (icon, name) = match st.library.get(*id) {
                    Some(w) if w.kind == MediaKind::Video => (icons::VIDEO, w.name.clone()),
                    Some(w) => (icons::IMAGE, w.name.clone()),
                    None => (icons::WARNING, t!("wallpapers.choice_missing").to_owned()),
                };
                ui.label(widgets::dim(format!("{}.", i + 1)));
                ui.label(format!("{icon}  {name}"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::icon_button(ui, icons::CLOSE, t!("wallpapers.remove_from_playlist"))
                        .clicked()
                    {
                        change = Some((i, 0));
                    }
                    if i > 0
                        && widgets::icon_button(ui, icons::CHEVRON_UP, t!("wallpapers.move_up"))
                            .clicked()
                    {
                        change = Some((i, -1));
                    }
                });
            });
        }
        if let Some((i, delta)) = change {
            let list = &mut m.settings.wallpaper.playlist.items;
            if delta == 0 {
                list.remove(i);
            } else {
                list.swap(i, i - 1);
            }
            m.save();
        }
        widgets::row_separator(ui);
        let mut order = m.settings.wallpaper.playlist.order;
        let changed = widgets::setting_row(ui, icons::SORT, t!("wallpapers.order"), None, |ui| {
            let shuffle = widgets::choice_value(
                ui,
                &mut order,
                PlaylistOrder::Shuffle,
                t!("wallpapers.order_shuffle"),
            );
            let in_order = widgets::choice_value(
                ui,
                &mut order,
                PlaylistOrder::InOrder,
                t!("wallpapers.order_in_order"),
            );
            shuffle.changed() || in_order.changed()
        });
        if changed {
            m.settings.wallpaper.playlist.order = order;
            m.save();
        }
        widgets::row_separator(ui);
        let mut minutes = m.settings.wallpaper.playlist.interval();
        let changed =
            widgets::setting_row(ui, icons::TIMER, t!("wallpapers.interval"), None, |ui| {
                ui.add(
                    egui::Slider::new(&mut minutes, PLAYLIST_MIN_MINUTES..=PLAYLIST_MAX_MINUTES)
                        .logarithmic(true)
                        .custom_formatter(|v, _| format_duration(v.round() as u32)),
                )
                .changed()
            });
        if changed {
            m.settings.wallpaper.playlist.interval_minutes = minutes;
            m.save_later();
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !items.is_empty(),
                    egui::Button::new(format!(
                        "{}  {}",
                        icons::PLAY,
                        t!("wallpapers.use_playlist")
                    )),
                )
                .clicked()
            {
                apply_choice(m, st, Choice::Playlist);
            }
        });
    });
}

fn sound(m: &mut Manager, ui: &mut Ui) {
    widgets::card(ui, |ui| {
        card_title(ui, icons::VOLUME, t!("wallpapers.sound_title"));
        let w = &mut m.settings.wallpaper;
        let mut muted = w.muted;
        let toggled = widgets::setting_row(
            ui,
            icons::MUTE,
            t!("wallpapers.muted"),
            Some(t!("wallpapers.muted_hint")),
            |ui| widgets::toggle(ui, &mut muted).changed(),
        );
        widgets::row_separator(ui);
        let mut volume = w.volume.min(100);
        let slid = widgets::setting_row(ui, icons::VOLUME, t!("wallpapers.volume"), None, |ui| {
            ui.add_enabled(!muted, egui::Slider::new(&mut volume, 0..=100).suffix(" %"))
                .changed()
        });
        w.muted = muted;
        w.volume = volume;
        if toggled {
            m.save();
        } else if slid {
            m.save_later();
        }
    });
}

fn smart_pause(m: &mut Manager, ui: &mut Ui) {
    widgets::card(ui, |ui| {
        card_title(ui, icons::PAUSE, t!("wallpapers.pause_title"));
        widgets::hint(ui, t!("wallpapers.pause_hint"));
        ui.add_space(4.0);
        let p = &mut m.settings.wallpaper.pause;
        let mut changed = false;
        changed |= widgets::setting_row(
            ui,
            icons::MONITOR,
            t!("wallpapers.pause_fullscreen"),
            None,
            |ui| widgets::toggle(ui, &mut p.on_fullscreen).changed(),
        );
        widgets::row_separator(ui);
        changed |= widgets::setting_row(
            ui,
            icons::POWER,
            t!("wallpapers.pause_battery"),
            None,
            |ui| widgets::toggle(ui, &mut p.on_battery).changed(),
        );
        let mut slid = false;
        if p.on_battery {
            let mut threshold = p.battery_threshold.clamp(10, 100);
            slid = widgets::setting_row(
                ui,
                "",
                t!("wallpapers.battery_threshold"),
                Some(t!("wallpapers.battery_threshold_hint")),
                |ui| {
                    ui.add(
                        egui::Slider::new(&mut threshold, 10..=100).custom_formatter(|v, _| {
                            if v >= 100.0 {
                                t!("wallpapers.battery_always").to_owned()
                            } else {
                                format!("{} %", v.round())
                            }
                        }),
                    )
                    .changed()
                },
            );
            p.battery_threshold = threshold;
        }
        widgets::row_separator(ui);
        changed |=
            widgets::setting_row(ui, icons::LOCK, t!("wallpapers.pause_locked"), None, |ui| {
                widgets::toggle(ui, &mut p.when_locked).changed()
            });
        widgets::row_separator(ui);
        changed |= widgets::setting_row(
            ui,
            icons::EYE_OFF,
            t!("wallpapers.pause_hidden"),
            None,
            |ui| widgets::toggle(ui, &mut p.when_desktop_hidden).changed(),
        );
        if changed {
            m.save();
        } else if slid {
            m.save_later();
        }
    });
}

fn online(m: &mut Manager, st: &mut PageState, ui: &mut Ui) {
    widgets::card(ui, |ui| {
        card_title(ui, icons::GLOBE, t!("wallpapers.online_title"));
        widgets::hint(ui, t!("wallpapers.online_hint"));
        ui.add_space(4.0);
        let mut bing = m.settings.wallpaper.online.bing;
        let changed = widgets::setting_row(
            ui,
            icons::IMAGE,
            t!("wallpapers.bing"),
            Some(t!("wallpapers.bing_hint")),
            |ui| widgets::toggle(ui, &mut bing).changed(),
        );
        if changed {
            m.settings.wallpaper.online.bing = bing;
            m.save();
        }
        ui.add_space(6.0);
        if ui
            .add_enabled(bing, egui::Button::new(t!("wallpapers.use_bing")))
            .clicked()
        {
            apply_choice(
                m,
                st,
                Choice::Online {
                    source: "bing".into(),
                },
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Au-delà de `MAX_TEXTURES`, les miniatures les moins récemment vues partent ;
    /// celles de l'image en cours restent.
    #[test]
    fn thumbnail_textures_are_bounded() {
        let ctx = egui::Context::default();
        let mut st = PageState {
            frame: 1000,
            ..Default::default()
        };
        let image = egui::ColorImage::new([2, 2], vec![Color32::BLACK; 4]);
        let mut ids = Vec::new();
        for k in 0..(MAX_TEXTURES as u64 + 10) {
            let id = Uuid::new_v4();
            let texture = ctx.load_texture("essai", image.clone(), egui::TextureOptions::LINEAR);
            st.textures.insert(id, (texture, k));
            st.requested.insert(id);
            ids.push(id);
        }
        st.trim_textures();
        assert_eq!(st.textures.len(), MAX_TEXTURES);
        assert!(!st.textures.contains_key(&ids[0]), "la plus ancienne part");
        assert!(
            !st.requested.contains(&ids[0]),
            "redemandée si elle revient"
        );
        assert!(st.textures.contains_key(ids.last().unwrap()));
    }
}
