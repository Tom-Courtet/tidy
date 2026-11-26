//! Fonds d'écran dans l'agent (lot J) : un fil qui applique les réglages.
//!
//! - **fonds fixes et couleurs** : posés par l'API du système (`tidy-platform`), sans
//!   fenêtre ni processus ;
//! - **fonds vidéo** : le processus `tidy --wallpaper` est lancé (et relancé s'il
//!   s'arrête, [`ChildProcess`]) tant qu'un écran affiche une vidéo ; il reçoit son état
//!   par `Event::Module { module: "wallpaper", action: "video_state" }` ;
//! - **playlist** : l'agent tient le fond courant et l'intervalle ;
//! - **pause intelligente** : l'état du système est relevé toutes les
//!   [`TICK`] ; en pause, la vidéo s'arrête et la playlist n'avance plus ;
//! - **fond d'origine** : relevé avant le premier changement
//!   (`wallpaper-original.json`), rendu quand les fonds de Tidy sont désactivés, sur
//!   demande, ou à la désinstallation ([`restore_original`]).
//!
//! Demandes reçues (`Request::Module { module: "wallpaper", action }`, voir
//! [`handle`]) : `reload` (bibliothèque modifiée), `next`, `restore_original`,
//! `status`, `video_state`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tidy_core::events::{Event, EventBus};
use tidy_core::screen::{Screen, ScreenId};
use tidy_core::settings::wallpaper::{Choice, OriginalWallpaper, WallpaperSettings};
use tidy_core::{Settings, Store};
use tidy_wallpaper::pause::{self, Conditions, PauseReason};
use tidy_wallpaper::playlist::PlaylistState;
use tidy_wallpaper::{Library, MediaKind, source, thumbs};

use super::supervisor::ChildProcess;
use crate::ipc::Response;
use crate::wallpaper::{VideoScreen, VideoState};

/// Fréquence des vérifications (pause, playlist, processus du lecteur).
const TICK: Duration = Duration::from_secs(2);
/// Relecture des écrans.
const SCREENS_EVERY: Duration = Duration::from_secs(10);
/// Nouvelle image d'une source en ligne cherchée au plus toutes les…
const ONLINE_EVERY: Duration = Duration::from_secs(6 * 3600);
/// Images gardées par source en ligne.
const ONLINE_KEEP: usize = 7;

enum Msg {
    Reload,
    Next,
    Restore(Sender<Result<(), String>>),
    Status(Sender<serde_json::Value>),
    VideoState(Sender<VideoState>),
    Bus(Event),
}

static CONTROL: OnceLock<Sender<Msg>> = OnceLock::new();

/// Lance le fil des fonds d'écran.
pub fn start(store: Store, bus: EventBus) {
    let (tx, rx) = mpsc::channel();
    if CONTROL.set(tx.clone()).is_err() {
        return;
    }
    let events = bus.subscribe();
    let forward = thread::Builder::new()
        .name("tidy-wallpaper-events".into())
        .spawn(move || {
            for event in events {
                let wanted = matches!(
                    event,
                    Event::SettingsChanged | Event::DesktopVisibility { .. } | Event::Shutdown
                );
                if wanted && tx.send(Msg::Bus(event)).is_err() {
                    return;
                }
            }
        });
    if let Err(e) = forward {
        log::error!("fil des événements des fonds d'écran : {e}");
    }
    let spawned = thread::Builder::new()
        .name("tidy-wallpaper".into())
        .spawn(move || Controller::new(store, bus).run(rx));
    if let Err(e) = spawned {
        log::error!("fil des fonds d'écran : {e}");
    }
}

/// Demande de la page Fonds d'écran ou du lecteur.
pub fn handle(action: &str) -> Response {
    let Some(tx) = CONTROL.get() else {
        return Response::Error {
            message: "fonds d'écran : pas démarrés".into(),
        };
    };
    let fail = || Response::Error {
        message: "fonds d'écran : pas de réponse".into(),
    };
    const WAIT: Duration = Duration::from_secs(15);
    match action {
        "reload" => {
            let _ = tx.send(Msg::Reload);
            Response::Ok
        }
        "next" => {
            let _ = tx.send(Msg::Next);
            Response::Ok
        }
        "restore_original" => {
            let (reply, rx) = mpsc::channel();
            let _ = tx.send(Msg::Restore(reply));
            match rx.recv_timeout(WAIT) {
                Ok(Ok(())) => Response::Ok,
                Ok(Err(message)) => Response::Error { message },
                Err(_) => fail(),
            }
        }
        "status" => {
            let (reply, rx) = mpsc::channel();
            let _ = tx.send(Msg::Status(reply));
            rx.recv_timeout(WAIT)
                .map_or_else(|_| fail(), |data| Response::Data { data })
        }
        "video_state" => {
            let (reply, rx) = mpsc::channel();
            let _ = tx.send(Msg::VideoState(reply));
            match rx.recv_timeout(WAIT) {
                Ok(state) => Response::Data {
                    data: serde_json::to_value(state).unwrap_or_default(),
                },
                Err(_) => fail(),
            }
        }
        other => Response::Error {
            message: format!("fonds d'écran : action inconnue « {other} »"),
        },
    }
}

/// Rend le fond d'origine noté avant le premier changement (désinstallation, page).
/// `Ok(false)` : rien à rendre.
pub fn restore_original(store: &Store) -> anyhow::Result<bool> {
    let path = store.path_of::<OriginalWallpaper>();
    if !path.exists() {
        return Ok(false);
    }
    let original: OriginalWallpaper = store.load();
    tidy_platform::current().restore_wallpaper(&original)?;
    let _ = std::fs::remove_file(&path);
    log::info!("fond d'écran d'origine rendu");
    Ok(true)
}

/// Ce qu'un écran doit afficher, une fois le choix résolu.
#[derive(Debug, Clone, PartialEq)]
enum Target {
    Nothing,
    Image(PathBuf),
    Color([u8; 3]),
    Video(PathBuf),
}

/// État résumé pour la page Fonds d'écran.
#[derive(Debug, Serialize)]
struct Status {
    enabled: bool,
    video_running: bool,
    gave_up: bool,
    paused: Option<PauseReason>,
    original_saved: bool,
    /// Secondes avant le prochain fond de la playlist.
    next_in: Option<u64>,
    error: Option<String>,
}

struct Controller {
    store: Store,
    bus: EventBus,
    settings: WallpaperSettings,
    library: Library,
    playlist: PlaylistState,
    screens: Vec<Screen>,
    screens_at: Instant,
    /// Dernier fond fixe posé, par écran (`"*"` : tous).
    applied: HashMap<String, Target>,
    video: VideoState,
    player: ChildProcess,
    paused: Option<PauseReason>,
    paused_since: Option<Instant>,
    desktop_hidden: bool,
    online_at: HashMap<String, Instant>,
    error: Option<String>,
}

impl Controller {
    fn new(store: Store, bus: EventBus) -> Self {
        let settings = store.load::<Settings>().wallpaper;
        let library = store.load();
        Self {
            store,
            bus,
            settings,
            library,
            playlist: PlaylistState::default(),
            screens: tidy_platform::current().screens(),
            screens_at: Instant::now(),
            applied: HashMap::new(),
            video: VideoState::default(),
            player: ChildProcess::new("--wallpaper"),
            paused: None,
            paused_since: None,
            desktop_hidden: false,
            online_at: HashMap::new(),
            error: None,
        }
    }

    fn run(mut self, rx: Receiver<Msg>) {
        self.apply();
        loop {
            match rx.recv_timeout(TICK) {
                Ok(Msg::Reload) => {
                    self.library = self.store.load();
                    self.apply();
                }
                Ok(Msg::Next) => {
                    let now = Instant::now();
                    self.playlist.advance(&self.settings.playlist, now);
                    self.apply();
                }
                Ok(Msg::Restore(reply)) => {
                    let result = self.disable_and_restore().map_err(|e| format!("{e:#}"));
                    let _ = reply.send(result);
                }
                Ok(Msg::Status(reply)) => {
                    let _ = reply.send(self.status());
                }
                Ok(Msg::VideoState(reply)) => {
                    let _ = reply.send(self.video.clone());
                }
                Ok(Msg::Bus(Event::SettingsChanged)) => self.reload_settings(),
                Ok(Msg::Bus(Event::DesktopVisibility { hidden })) => {
                    self.desktop_hidden = hidden;
                    self.tick();
                }
                Ok(Msg::Bus(Event::Shutdown)) | Err(RecvTimeoutError::Disconnected) => {
                    self.player.stop();
                    return;
                }
                Ok(Msg::Bus(_)) => {}
                Err(RecvTimeoutError::Timeout) => self.tick(),
            }
        }
    }

    fn status(&self) -> serde_json::Value {
        let status = Status {
            enabled: self.settings.enabled,
            video_running: self.player.running(),
            gave_up: self.player.gave_up(),
            paused: self.paused,
            original_saved: self.store.path_of::<OriginalWallpaper>().exists(),
            next_in: self
                .settings
                .uses_playlist()
                .then(|| {
                    self.playlist
                        .remaining(&self.settings.playlist, Instant::now())
                })
                .flatten()
                .map(|d| d.as_secs()),
            error: self.error.clone(),
        };
        serde_json::to_value(status).unwrap_or_default()
    }

    fn reload_settings(&mut self) {
        let new = self.store.load::<Settings>().wallpaper;
        if new == self.settings {
            return;
        }
        let was_enabled = self.settings.enabled;
        if (new.fit, new.background) != (self.settings.fit, self.settings.background) {
            // Même image, autre ajustement : à reposer.
            self.applied.clear();
        }
        self.settings = new;
        self.player.reset();
        if was_enabled && !self.settings.enabled {
            if let Err(e) = self.disable_and_restore() {
                log::warn!("fond d'origine : {e:#}");
            }
            return;
        }
        self.apply();
    }

    /// Arrête tout et rend le fond d'origine.
    fn disable_and_restore(&mut self) -> anyhow::Result<()> {
        self.stop_video();
        self.applied.clear();
        self.error = None;
        restore_original(&self.store)?;
        Ok(())
    }

    fn stop_video(&mut self) {
        if self.player.running() {
            self.player.stop();
            tidy_platform::current().refresh_wallpaper();
        }
        self.video.screens.clear();
    }

    fn refresh_screens(&mut self) -> bool {
        if self.screens_at.elapsed() < SCREENS_EVERY {
            return false;
        }
        self.screens_at = Instant::now();
        let screens = tidy_platform::current().screens();
        let changed = screens.len() != self.screens.len()
            || screens
                .iter()
                .zip(&self.screens)
                .any(|(a, b)| a.id != b.id || a.frame != b.frame);
        if changed {
            log::info!("écrans changés : fonds d'écran réappliqués");
            self.screens = screens;
            self.applied.clear();
        }
        changed
    }

    fn tick(&mut self) {
        if !self.settings.enabled {
            return;
        }
        let mut dirty = self.refresh_screens();
        // Pause intelligente.
        let activity = tidy_platform::current().system_activity();
        let reason = pause::reason(
            &self.settings.pause,
            &Conditions {
                fullscreen: activity.fullscreen,
                on_battery: activity.on_battery,
                battery_percent: activity.battery_percent,
                locked: activity.locked,
                desktop_hidden: self.desktop_hidden,
            },
        );
        if reason != self.paused {
            log::info!(
                "fonds d'écran : {}",
                match reason {
                    Some(r) => format!("pause ({r:?})"),
                    None => "reprise".into(),
                }
            );
            match (self.paused, reason) {
                (None, Some(_)) => self.paused_since = Some(Instant::now()),
                (Some(_), None) => {
                    if let Some(since) = self.paused_since.take() {
                        self.playlist.postpone(since.elapsed());
                    }
                }
                _ => {}
            }
            self.paused = reason;
            dirty = true;
        }
        // Playlist.
        let now = Instant::now();
        if self.paused.is_none()
            && self.settings.uses_playlist()
            && self.playlist.due(&self.settings.playlist, now)
        {
            self.playlist.advance(&self.settings.playlist, now);
            dirty = true;
        }
        // Lecteur arrêté de lui-même : relancé (ou abandon).
        if self.player.check() && self.player.gave_up() {
            self.error = Some("le lecteur vidéo s'arrête sans cesse".into());
        }
        self.fetch_online();
        if dirty {
            self.apply();
        }
    }

    /// Image du jour des sources en ligne utilisées (si l'utilisateur les a activées),
    /// sur un fil à part : la réponse peut prendre du temps.
    fn fetch_online(&mut self) {
        let mut used: Vec<String> = Vec::new();
        let choices = std::iter::once(&self.settings.all)
            .chain(self.settings.screens.iter().map(|s| &s.choice));
        for choice in choices {
            if let Choice::Online { source } = choice
                && !used.contains(source)
            {
                used.push(source.clone());
            }
        }
        for id in used {
            let allowed = id == "bing" && self.settings.online.bing;
            let recent = self
                .online_at
                .get(&id)
                .is_some_and(|t| t.elapsed() < ONLINE_EVERY);
            if !allowed || recent {
                continue;
            }
            self.online_at.insert(id.clone(), Instant::now());
            let root = self.store.root().to_owned();
            let _ = thread::Builder::new()
                .name("tidy-wallpaper-online".into())
                .spawn(move || {
                    let Some(source) = source::by_id(&id) else {
                        return;
                    };
                    let dir = source::download_dir(&root, &id);
                    match source.fetch(&dir) {
                        Ok(_) => {
                            source::prune(&root, &id, ONLINE_KEEP);
                            if let Some(tx) = CONTROL.get() {
                                let _ = tx.send(Msg::Reload);
                            }
                        }
                        Err(e) => log::warn!("source en ligne « {id} » : {e:#}"),
                    }
                });
        }
    }

    /// Ce que doit afficher un choix.
    fn resolve(&mut self, choice: &Choice) -> Target {
        let root = self.store.root().to_owned();
        let item = |library: &Library, id| -> Target {
            let Some(w) = library.get(id) else {
                return Target::Nothing;
            };
            if !w.available() {
                return Target::Nothing;
            }
            match w.kind {
                MediaKind::Video => Target::Video(w.path.clone()),
                MediaKind::Image => {
                    let path = thumbs::system_image(&root, w);
                    if path.is_file() {
                        Target::Image(path)
                    } else {
                        // Image à convertir (WebP) : faite par la page à l'import ;
                        // à défaut, l'original.
                        Target::Image(w.path.clone())
                    }
                }
            }
        };
        match choice {
            Choice::None => Target::Nothing,
            Choice::Item { id } => item(&self.library, *id),
            Choice::Color { rgb } => Target::Color(*rgb),
            Choice::Playlist => {
                match self
                    .playlist
                    .current(&self.settings.playlist, Instant::now())
                {
                    Some(id) => item(&self.library, id),
                    None => Target::Nothing,
                }
            }
            Choice::Online { source } => {
                let allowed = source == "bing" && self.settings.online.bing;
                match source::latest(&root, source) {
                    Some(path) if allowed => Target::Image(path),
                    _ => Target::Nothing,
                }
            }
        }
    }

    /// Note le fond d'origine avant le premier changement.
    fn ensure_original(&self) {
        let path = self.store.path_of::<OriginalWallpaper>();
        if path.exists() {
            return;
        }
        let Some(mut original) = tidy_platform::current().wallpaper_snapshot() else {
            log::warn!("fond d'écran d'origine illisible : il ne pourra pas être rendu");
            return;
        };
        secure_original(self.store.root(), &mut original);
        match self.store.save(&original) {
            Ok(()) => log::info!("fond d'écran d'origine noté"),
            Err(e) => log::error!("fond d'origine non enregistré : {e:#}"),
        }
    }

    /// Applique les réglages : fonds fixes et état du lecteur vidéo.
    fn apply(&mut self) {
        if !self.settings.enabled {
            self.stop_video();
            return;
        }
        let platform = tidy_platform::current();
        let screens = self.screens.clone();
        // (clé, écran visé, cible)
        let mut targets: Vec<(String, Option<ScreenId>, Target, Option<&Screen>)> = Vec::new();
        if self.settings.same_on_all {
            let choice = self.settings.all.clone();
            let target = self.resolve(&choice);
            targets.push(("*".into(), None, target, None));
        } else {
            for screen in &screens {
                let choice = self.settings.choice_for(&screen.id.0).clone();
                let target = self.resolve(&choice);
                targets.push((
                    screen.id.0.clone(),
                    Some(screen.id.clone()),
                    target,
                    Some(screen),
                ));
            }
        }
        let changes = targets
            .iter()
            .any(|(key, _, t, _)| *t != Target::Nothing && self.applied.get(key) != Some(t));
        if changes {
            self.ensure_original();
        }
        let mut videos = Vec::new();
        let mut error = None;
        let (fit, background) = (self.settings.fit, self.settings.background);
        for (key, screen, target, frame) in targets {
            match &target {
                Target::Nothing => {}
                Target::Video(path) => {
                    let on: Vec<&Screen> = match frame {
                        Some(s) => vec![s],
                        None => screens.iter().collect(),
                    };
                    for s in on {
                        videos.push(VideoScreen {
                            screen: s.id.0.clone(),
                            frame: s.frame,
                            path: path.clone(),
                        });
                    }
                }
                Target::Image(_) | Target::Color(_) if self.applied.get(&key) == Some(&target) => {}
                Target::Image(path) => {
                    match platform.set_wallpaper(screen.as_ref(), path, fit, background) {
                        Ok(()) => {
                            log::info!("fond d'écran posé : {}", path.display());
                            self.applied.insert(key, target.clone());
                        }
                        Err(e) => error = Some(format!("{e:#}")),
                    }
                }
                Target::Color(rgb) => {
                    let result = thumbs::solid_color(self.store.root(), *rgb).and_then(|image| {
                        platform.set_wallpaper(
                            screen.as_ref(),
                            &image,
                            tidy_core::settings::wallpaper::Fit::Stretch,
                            *rgb,
                        )
                    });
                    match result {
                        Ok(()) => {
                            self.applied.insert(key, target.clone());
                        }
                        Err(e) => error = Some(format!("{e:#}")),
                    }
                }
            }
        }
        if let Some(e) = &error {
            log::warn!("fond d'écran : {e}");
        }
        self.error = error;
        // Vidéos.
        let state = VideoState {
            screens: videos,
            fit,
            background,
            volume: self.settings.volume.min(100),
            muted: self.settings.muted,
            paused: self.paused.is_some(),
        };
        if state.screens.is_empty() {
            self.stop_video();
            self.video = state;
            return;
        }
        if state != self.video {
            self.video = state.clone();
            self.bus.publish(Event::Module {
                module: "wallpaper".into(),
                action: "video_state".into(),
                data: serde_json::to_value(state).unwrap_or_default(),
            });
        }
        self.player.ensure();
    }
}

/// Windows garde parfois le fond courant dans un fichier à lui
/// (`…\Themes\TranscodedWallpaper`), qu'il réécrira au prochain changement : on en
/// garde une copie, pour pouvoir le rendre.
fn secure_original(data_root: &Path, original: &mut OriginalWallpaper) {
    let dir = tidy_wallpaper::cache_dir(data_root).join("original");
    for (i, s) in original.screens.iter_mut().enumerate() {
        let Some(image) = &s.image else {
            continue;
        };
        let name = image
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let in_themes = image
            .components()
            .any(|c| c.as_os_str().eq_ignore_ascii_case("Themes"));
        if !(name.starts_with("transcodedwallpaper") || in_themes) || !image.is_file() {
            continue;
        }
        let ext = image
            .extension()
            .map_or("jpg".into(), |e| e.to_string_lossy().into_owned());
        let copy = dir.join(format!("original-{i}.{ext}"));
        let saved = std::fs::create_dir_all(&dir).and_then(|()| std::fs::copy(image, &copy));
        match saved {
            Ok(_) => s.image = Some(copy),
            Err(e) => log::warn!("copie du fond d'origine impossible : {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use tidy_core::settings::wallpaper::OriginalScreen;

    use super::*;

    #[test]
    fn transcoded_originals_are_copied() {
        let dir = tempfile::tempdir().unwrap();
        let themes = dir.path().join("Themes");
        std::fs::create_dir_all(&themes).unwrap();
        let transcoded = themes.join("TranscodedWallpaper");
        std::fs::write(&transcoded, b"jpeg").unwrap();
        let photo = dir.path().join("photo.jpg");
        std::fs::write(&photo, b"jpeg").unwrap();
        let mut original = OriginalWallpaper {
            screens: vec![
                OriginalScreen {
                    screen: "a".into(),
                    image: Some(transcoded.clone()),
                    fit: None,
                    background: None,
                },
                OriginalScreen {
                    screen: "b".into(),
                    image: Some(photo.clone()),
                    fit: None,
                    background: None,
                },
            ],
            ..Default::default()
        };
        let data = dir.path().join("données");
        secure_original(&data, &mut original);
        let copy = original.screens[0].image.clone().unwrap();
        assert!(copy.starts_with(&data));
        assert!(copy.is_file());
        assert!(transcoded.is_file(), "l'original du système reste");
        assert_eq!(original.screens[1].image.as_ref(), Some(&photo));
    }

    #[test]
    fn nothing_to_restore_without_a_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        assert!(!restore_original(&store).unwrap());
    }
}
