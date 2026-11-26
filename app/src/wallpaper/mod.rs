//! Fonds d'écran animés (`tidy --wallpaper`, lot J) : processus lancé et surveillé
//! par l'agent tant qu'un écran affiche une vidéo.
//!
//! Une fenêtre par écran concerné, **derrière les icônes du bureau** (Windows : enfant
//! du `WorkerW` de l'Explorateur ; macOS : niveau du fond d'écran), et le lecteur
//! vidéo du système dedans ([`tidy_wallpaper::player`]). Pas d'egui ni d'OpenGL : la
//! vidéo est décodée et affichée par le système, ce processus ne fait que tenir les
//! fenêtres.
//!
//! L'état vient de l'agent ([`VideoState`]) : demandé au démarrage
//! (`Request::Module { module: "wallpaper", action: "video_state" }`), puis poussé à
//! chaque changement (`Event::Module`, même action). La fin du flux d'événements
//! (agent arrêté) ferme le processus.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tidy_core::Store;
use tidy_core::events::Event;
use tidy_core::geometry::Rect;
use tidy_core::settings::wallpaper::Fit;
use tidy_wallpaper::player::{Player, VideoOptions};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::ipc::{self, Request, Response};

/// Vidéo affichée sur un écran.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoScreen {
    /// Identifiant de l'écran (`ScreenId`).
    pub screen: String,
    /// Cadre de l'écran, en points.
    pub frame: Rect,
    pub path: PathBuf,
}

/// Ce que doit montrer le lecteur.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoState {
    pub screens: Vec<VideoScreen>,
    pub fit: Fit,
    pub background: [u8; 3],
    /// Volume de 0 à 100.
    pub volume: u8,
    pub muted: bool,
    pub paused: bool,
}

impl Default for VideoState {
    fn default() -> Self {
        Self {
            screens: Vec::new(),
            fit: Fit::default(),
            background: [0, 0, 0],
            volume: 50,
            muted: true,
            paused: false,
        }
    }
}

impl VideoState {
    /// Réglages du lecteur de l'écran n° `index` : le son ne sort que d'un écran (le
    /// premier), même si la vidéo est partout.
    pub fn options(&self, index: usize) -> VideoOptions {
        VideoOptions {
            fit: self.fit,
            background: self.background,
            volume: f32::from(self.volume.min(100)) / 100.0,
            muted: self.muted || index > 0,
        }
    }
}

/// Titre des fenêtres (retrouvées par le système).
const TITLE: &str = "Tidy · fond d'écran";
/// Intervalle des vérifications du lecteur (reprise en boucle sous Windows).
const TICK: Duration = Duration::from_millis(250);

enum Msg {
    State(VideoState),
    Quit,
}

pub fn run(store: Store) -> Result<()> {
    let state = match ipc::request(
        &store,
        &Request::Module {
            module: "wallpaper".into(),
            action: "video_state".into(),
            data: serde_json::Value::Null,
        },
    ) {
        Ok(Response::Data { data }) => {
            serde_json::from_value::<VideoState>(data).context("état du fond illisible")?
        }
        Ok(other) => bail!("réponse inattendue de l'agent : {other:?}"),
        Err(e) => bail!("agent injoignable : {e:#}"),
    };

    let mut builder = EventLoop::<Msg>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder
            .with_activation_policy(ActivationPolicy::Accessory)
            .with_default_menu(false);
    }
    let event_loop = builder.build().context("boucle d'événements")?;
    let proxy = event_loop.create_proxy();
    let end = event_loop.create_proxy();
    ipc::subscribe(
        &store,
        move |event| match event {
            Event::Module {
                module,
                action,
                data,
            } if module == "wallpaper" && action == "video_state" => {
                match serde_json::from_value(data) {
                    Ok(state) => {
                        let _ = proxy.send_event(Msg::State(state));
                    }
                    Err(e) => log::warn!("état du fond illisible : {e}"),
                }
            }
            Event::Shutdown => {
                let _ = proxy.send_event(Msg::Quit);
            }
            _ => {}
        },
        move || {
            let _ = end.send_event(Msg::Quit);
        },
    )?;

    let mut app = App {
        state,
        surfaces: HashMap::new(),
    };
    event_loop
        .run_app(&mut app)
        .context("boucle d'événements")?;
    log::info!("lecteur des fonds animés arrêté");
    Ok(())
}

/// Fenêtre d'un écran et son lecteur.
struct Surface {
    window: Window,
    player: Option<Player>,
    frame: Rect,
    path: PathBuf,
}

struct App {
    state: VideoState,
    /// Par identifiant d'écran.
    surfaces: HashMap<String, Surface>,
}

impl App {
    /// Fenêtres et lecteurs conformes à l'état.
    fn sync(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.screens.is_empty() {
            event_loop.exit();
            return;
        }
        self.surfaces
            .retain(|id, _| self.state.screens.iter().any(|s| s.screen == *id));
        for (index, wanted) in self.state.screens.iter().enumerate() {
            let options = self.state.options(index);
            let current = self
                .surfaces
                .get(&wanted.screen)
                .is_some_and(|s| s.frame == wanted.frame && s.path == wanted.path);
            if !current {
                // Fenêtre recréée : la vidéo ou l'écran a changé.
                self.surfaces.remove(&wanted.screen);
                match create_surface(event_loop, wanted, options) {
                    Ok(surface) => {
                        self.surfaces.insert(wanted.screen.clone(), surface);
                    }
                    Err(e) => log::error!("fond animé sur {} : {e:#}", wanted.screen),
                }
            }
            if let Some(player) = self
                .surfaces
                .get_mut(&wanted.screen)
                .and_then(|s| s.player.as_mut())
            {
                player.set_options(options);
                player.set_paused(self.state.paused);
            }
        }
    }
}

fn create_surface(
    event_loop: &ActiveEventLoop,
    wanted: &VideoScreen,
    options: VideoOptions,
) -> Result<Surface> {
    let f = wanted.frame;
    let attributes = Window::default_attributes()
        .with_title(TITLE)
        .with_decorations(false)
        .with_resizable(false)
        .with_position(LogicalPosition::new(f64::from(f.x), f64::from(f.y)))
        .with_inner_size(LogicalSize::new(f64::from(f.width), f64::from(f.height)));
    let window = event_loop
        .create_window(attributes)
        .context("fenêtre du fond")?;
    if !tidy_platform::current().attach_wallpaper_window(&window, f) {
        bail!("fenêtre impossible à placer derrière les icônes");
    }
    // Les clics traversent vers le bureau (macOS ; sous Windows la fenêtre est déjà
    // sous les icônes).
    #[cfg(target_os = "macos")]
    let _ = window.set_cursor_hittest(false);
    let player = Player::new(&window, &wanted.path, options)
        .inspect_err(|e| log::error!("lecture de {} : {e:#}", wanted.path.display()))
        .ok();
    log::info!(
        "fond animé sur {} : {}",
        wanted.screen,
        wanted.path.display()
    );
    Ok(Surface {
        window,
        player,
        frame: f,
        path: wanted.path.clone(),
    })
}

impl ApplicationHandler<Msg> for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        match cause {
            StartCause::Init => self.sync(event_loop),
            StartCause::ResumeTimeReached { .. } => {
                for s in self.surfaces.values_mut() {
                    if let Some(p) = &mut s.player {
                        p.tick();
                    }
                }
            }
            _ => {}
        }
        // Sous Windows, le lecteur demande un coup d'œil régulier (fin de vidéo) ; sous
        // macOS, AVFoundation boucle seul : on dort.
        if cfg!(windows) {
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + TICK));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, msg: Msg) {
        match msg {
            Msg::State(state) => {
                self.state = state;
                self.sync(event_loop);
            }
            Msg::Quit => event_loop.exit(),
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(surface) = self.surfaces.values_mut().find(|s| s.window.id() == id) else {
            return;
        };
        match event {
            WindowEvent::Resized(_) | WindowEvent::RedrawRequested => {
                if let Some(p) = &mut surface.player {
                    p.resized();
                }
            }
            // Pas de fermeture par l'utilisateur : c'est l'agent qui décide.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_comes_from_one_screen_only() {
        let state = VideoState {
            muted: false,
            volume: 40,
            ..Default::default()
        };
        assert!(!state.options(0).muted);
        assert!(state.options(1).muted);
        assert!((state.options(0).volume - 0.4).abs() < 1e-6);
        assert!(VideoState::default().options(0).muted, "muet par défaut");
    }

    #[test]
    fn state_round_trips_as_json() {
        let state = VideoState {
            screens: vec![VideoScreen {
                screen: "écran".into(),
                frame: Rect::new(0.0, 0.0, 1920.0, 1080.0),
                path: PathBuf::from("/tmp/v.mp4"),
            }],
            paused: true,
            ..Default::default()
        };
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(serde_json::from_value::<VideoState>(json).unwrap(), state);
    }
}
