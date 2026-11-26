//! Agent : discret dans la barre des menus, il tient l'état du bureau, les raccourcis
//! globaux et surveille le processus du bureau.
//!
//! Répartition :
//! - **thread principal** : boucle winit sans fenêtre (pas d'icône dans le Dock),
//!   icône de la barre des menus ([`tray`]) et raccourcis ([`hotkeys`]), que macOS
//!   veut sur ce thread ;
//! - **fil du socket** ([`crate::ipc`]) : requêtes des autres processus, abonnements
//!   aux événements ;
//! - **fil de surveillance** ([`supervisor`]) : processus du bureau, icônes du Finder.

mod hotkeys;
mod layouts;
pub mod modules;
pub mod organizer;
pub mod search;
mod supervisor;
mod tray;
pub mod wallpaper;
mod watcher;

use std::path::PathBuf;
use std::process::{Child, Command as Process};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use tidy_core::events::{Event, EventBus};
use tidy_core::settings::{DesktopMode, QuickAccessMode, QuickHideScope};
use tidy_core::{Settings, Store};
use tidy_ui::t;
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

use crate::ipc::{self, Request, Response, Status};
use hotkeys::{Action, Hotkeys};
#[cfg(test)]
pub(crate) use supervisor::desktop_quit_event;
use supervisor::{Command, Shared, Supervisor};
use tray::{MenuChoice, Tray, TrayView};

/// Événements de la boucle principale.
#[derive(Debug, Clone, PartialEq, Eq)]
enum UserEvent {
    Menu(MenuChoice),
    Hotkey(Action, bool),
    /// Ouvrir la fenêtre principale, sur une page si donnée.
    OpenManager(Option<String>),
    ReloadSettings,
    ToggleDesktop,
    RestoreFinder,
    /// Les dispositions nommées ont changé : raccourcis et menu à refaire.
    LayoutsChanged,
    Quit,
}

/// Lance l'agent ; rend la main quand on le quitte. `open_manager` : ouvrir aussi la
/// fenêtre principale (lancement à la main, pas à l'ouverture de session).
pub fn run(store: Store, lock: ipc::InstanceLock, open_manager: bool) -> Result<()> {
    let (listener, auth) = ipc::bind(&store, &lock)?;
    let _socket_guard = SocketGuard(store.socket_path());
    let settings: Settings = store.load();
    crate::lang::apply(settings.language);

    let mut builder = EventLoop::<UserEvent>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        // Pas d'icône dans le Dock ni de menu d'application : seulement la barre des menus.
        builder
            .with_activation_policy(ActivationPolicy::Accessory)
            .with_default_menu(false);
    }
    let event_loop = builder.build().context("boucle d'événements")?;
    let proxy = event_loop.create_proxy();

    let bus = EventBus::default();
    let shared = Arc::new(Mutex::new(Shared::default()));
    let desktop_hidden = Arc::new(Mutex::new(false));
    let supervisor = Supervisor::spawn(store.clone(), bus.clone(), Arc::clone(&shared))?;
    supervisor.send(Command::Apply(settings.effective_desktop_mode()));
    organizer::start(store.clone(), bus.clone(), Arc::clone(&shared));
    {
        let proxy = proxy.clone();
        layouts::start(store.clone(), bus.clone(), move || {
            let _ = proxy.send_event(UserEvent::LayoutsChanged);
        });
    }
    wallpaper::start(store.clone(), bus.clone());

    let handler = {
        let proxy = proxy.clone();
        let bus = bus.clone();
        let store = store.clone();
        let shared = Arc::clone(&shared);
        let hidden = Arc::clone(&desktop_hidden);
        let supervisor = supervisor.clone();
        move |req: Request| {
            let send = |e| proxy.send_event(e).map(|()| Response::Ok);
            let result = match req {
                Request::Ping | Request::Subscribe => Ok(Response::Ok),
                Request::Status => Ok(Response::Status(Status {
                    version: env!("CARGO_PKG_VERSION").into(),
                    desktop_running: shared.lock().is_ok_and(|s| s.desktop_running),
                    finder_hidden: tidy_platform::finder::hidden_by_tidy(
                        &store.finder_marker_path(),
                    ),
                    desktop_hidden: hidden.lock().is_ok_and(|h| *h),
                })),
                Request::OpenManager { page } => send(UserEvent::OpenManager(page)),
                Request::ReloadSettings => send(UserEvent::ReloadSettings),
                Request::ReloadDesktop => {
                    bus.publish(Event::DesktopChanged);
                    Ok(Response::Ok)
                }
                Request::ToggleDesktop => send(UserEvent::ToggleDesktop),
                Request::CreateBox => {
                    bus.publish(Event::CreateBox { at: None });
                    Ok(Response::Ok)
                }
                Request::Desktop { command } => Ok(organizer::route(&store, &bus, command)),
                Request::ApplyRules { all } => Ok(organizer::apply_rules(&store, &bus, all)),
                Request::UndoOrganize => Ok(organizer::undo(&store, &bus)),
                Request::RestoreFinder => send(UserEvent::RestoreFinder),
                Request::DesktopReady => {
                    // Commandes reçues pendant le démarrage du bureau : relayées
                    // maintenant qu'il est abonné.
                    organizer::desktop_ready(&bus, &shared);
                    supervisor.send(Command::DesktopReady);
                    Ok(Response::Ok)
                }
                Request::Quit => send(UserEvent::Quit),
                Request::Module {
                    module,
                    action,
                    data,
                } => Ok(modules::handle(&store, &bus, module, action, data)),
            };
            result.unwrap_or_else(|_| Response::Error {
                message: "agent en cours d'arrêt".into(),
            })
        }
    };
    ipc::serve(listener, auth, bus.clone(), handler)?;

    let mut app = App {
        store,
        settings,
        proxy,
        bus,
        supervisor,
        desktop_hidden,
        tray: None,
        hotkeys: None,
        manager: None,
        open_manager_at_start: open_manager,
        quick_access: false,
    };
    event_loop
        .run_app(&mut app)
        .context("boucle d'événements")?;
    log::info!("agent arrêté");
    drop(lock);
    Ok(())
}

/// Retire le fichier du socket quand l'agent s'arrête.
struct SocketGuard(PathBuf);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Partie « thread principal » de l'agent.
struct App {
    store: Store,
    settings: Settings,
    proxy: EventLoopProxy<UserEvent>,
    bus: EventBus,
    supervisor: Supervisor,
    desktop_hidden: Arc<Mutex<bool>>,
    tray: Option<Tray>,
    hotkeys: Option<Hotkeys>,
    /// Fenêtre principale, si elle est ouverte.
    manager: Option<Child>,
    open_manager_at_start: bool,
    /// Accès rapide aux boîtes en cours.
    quick_access: bool,
}

impl App {
    /// Icône, raccourcis et réouverture, une fois la boucle lancée (exigence de macOS).
    fn start(&mut self) {
        self.start_tray();
        self.register_hotkeys();
        let platform = tidy_platform::current();
        // Icônes laissées masquées par un arrêt brutal précédent.
        if platform.desktop_icons_hidden() {
            let _ = platform.set_desktop_icons_hidden(false);
        }
        self.install_desktop_menu();
        // Icônes rangées : les masquer, rendre celles qui ne le sont plus (y compris
        // après un plantage). À part : l'Explorateur peut répondre lentement.
        let store = self.store.clone();
        std::thread::spawn(move || {
            crate::icons::log_report(crate::icons::sync(&store, &[]), false);
        });
        if self.settings.quick_actions.double_click_to_hide {
            let proxy = std::sync::Mutex::new(self.proxy.clone());
            let watching = platform.watch_desktop_double_click(Box::new(move || {
                if let Ok(proxy) = proxy.lock() {
                    let _ = proxy.send_event(UserEvent::ToggleDesktop);
                }
            }));
            log::info!(
                "double-clic sur le bureau : {}",
                if watching {
                    "surveillé"
                } else {
                    "indisponible"
                }
            );
        }
        let proxy = self.proxy.clone();
        tidy_platform::on_reopen(move || {
            let _ = proxy.send_event(UserEvent::OpenManager(None));
        });
        if self.open_manager_at_start {
            self.open_manager(None);
        }
        log::info!("agent prêt (données : {})", self.store.root().display());
    }

    /// Entrée « Créer une boîte » du menu contextuel du bureau (Windows).
    fn install_desktop_menu(&self) {
        let label =
            (self.settings.desktop_mode != DesktopMode::Off).then(|| t!("desktop.menu_create_box"));
        // Vers la copie installée si elle existe (lot K), même si cet agent a été lancé
        // depuis un exécutable téléchargé.
        let result = match (label, crate::install::Layout::from_env()) {
            (Some(label), Some(layout)) if layout.exe.is_file() => {
                tidy_platform::install::set_desktop_menu_for(&layout.exe, label)
            }
            (label, _) => tidy_platform::current().set_desktop_menu(label),
        };
        if let Err(e) = result {
            log::warn!("menu du bureau : {e:#}");
        }
    }

    fn register_hotkeys(&mut self) {
        // L'ancien jeu d'abord : un même raccourci ne s'enregistre pas deux fois.
        self.hotkeys = None;
        let proxy = self.proxy.clone();
        let layouts = layouts::hotkeys(&self.store);
        match Hotkeys::register(&self.settings.hotkeys, &layouts, move |action, down| {
            let _ = proxy.send_event(UserEvent::Hotkey(action, down));
        }) {
            Ok(h) => self.hotkeys = Some(h),
            Err(e) => log::error!("raccourcis globaux indisponibles : {e:#}"),
        }
    }

    fn refresh_tray(&mut self) {
        let view = TrayView {
            boxes_enabled: self.settings.desktop_mode != DesktopMode::Off,
            desktop_hidden: self.desktop_hidden.lock().is_ok_and(|h| *h),
            layouts: layouts::names(&self.store),
            wallpaper_playlist: self.settings.wallpaper.enabled
                && self.settings.wallpaper.uses_playlist(),
        };
        if let Some(tray) = &mut self.tray {
            tray.show(view);
        }
    }

    fn open_manager(&mut self, page: Option<String>) {
        if let Some(child) = &mut self.manager
            && matches!(child.try_wait(), Ok(None))
        {
            // Une fenêtre cachée derrière d'autres ne se redessine pas : c'est le
            // système qui la ramène devant ; l'événement change de page (il est traité
            // même fenêtre cachée).
            tidy_platform::activate_process(child.id());
            self.bus.publish(Event::FocusManager { page });
            return;
        }
        let spawned = std::env::current_exe().and_then(|exe| {
            let mut cmd = Process::new(exe);
            cmd.arg("--manager");
            if let Some(page) = &page {
                cmd.arg(page);
            }
            cmd.spawn()
        });
        match spawned {
            Ok(child) => self.manager = Some(child),
            Err(e) => log::error!("fenêtre principale impossible : {e}"),
        }
    }

    fn reload_settings(&mut self) {
        let old = std::mem::replace(&mut self.settings, self.store.load());
        crate::lang::apply(self.settings.language);
        if old.hotkeys != self.settings.hotkeys {
            self.register_hotkeys();
        }
        if old.effective_desktop_mode() != self.settings.effective_desktop_mode() {
            self.supervisor
                .send(Command::Apply(self.settings.effective_desktop_mode()));
        }
        if old.launch_at_login != self.settings.launch_at_login {
            // Windows : vers la copie installée si elle existe (lot K).
            let result = match crate::install::Layout::from_env() {
                Some(layout) if self.settings.launch_at_login && layout.exe.is_file() => {
                    tidy_platform::install::set_login_for(&layout.exe)
                }
                _ => tidy_platform::current()
                    .set_login_item(self.settings.launch_at_login)
                    .map(|_| ()),
            };
            if let Err(e) = result {
                log::warn!("ouverture de session : {e:#}");
            }
        }
        self.bus.publish(Event::SettingsChanged);
        if old.language != self.settings.language || old.desktop_mode != self.settings.desktop_mode
        {
            self.install_desktop_menu();
        }
        if old.language != self.settings.language {
            // Les libellés du menu changent : on recrée l'icône.
            self.tray = None;
            self.start_tray();
        } else {
            self.refresh_tray();
        }
    }

    fn start_tray(&mut self) {
        let proxy = self.proxy.clone();
        match Tray::new(move |choice| {
            let _ = proxy.send_event(UserEvent::Menu(choice));
        }) {
            Ok(tray) => self.tray = Some(tray),
            Err(e) => log::error!("icône de la barre des menus impossible : {e:#}"),
        }
        self.refresh_tray();
    }

    fn save_settings(&mut self, change: impl FnOnce(&mut Settings)) {
        let mut settings = match self.store.load_checked::<Settings>() {
            Ok(settings) => settings,
            Err(e) => {
                // Ne pas remplacer des réglages illisibles par les valeurs par défaut.
                log::error!("réglages non enregistrés : {e:#}");
                return;
            }
        };
        change(&mut settings);
        if let Err(e) = self.store.save(&settings) {
            log::error!("réglages non enregistrés : {e:#}");
        }
        self.reload_settings();
    }

    fn toggle_desktop(&mut self) {
        let hidden = {
            let mut h = self
                .desktop_hidden
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            *h = !*h;
            *h
        };
        self.bus.publish(Event::DesktopVisibility { hidden });
        // Les icônes du système aussi, si les réglages le demandent (Windows).
        if self.settings.quick_actions.hide_scope == QuickHideScope::Everything
            && let Err(e) = tidy_platform::current().set_desktop_icons_hidden(hidden)
        {
            log::debug!("icônes du bureau : {e:#}");
        }
        self.refresh_tray();
    }

    fn quit(&mut self, event_loop: &ActiveEventLoop) {
        log::info!("arrêt demandé");
        let platform = tidy_platform::current();
        // Avant toute attente (bureau, jusqu'à SHUTDOWN_GRACE ; Explorateur) : le
        // crochet souris global porté par ce thread ne répondrait plus.
        platform.stop_desktop_watch();
        self.bus.publish(Event::Shutdown);
        self.supervisor.shutdown();
        if platform.desktop_icons_hidden() {
            let _ = platform.set_desktop_icons_hidden(false);
        }
        crate::icons::restore_all(&self.store);
        // Raccourcis et icône retirés avant de rendre la main.
        self.hotkeys = None;
        self.tray = None;
        event_loop.exit();
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if cause == StartCause::Init {
            event_loop.set_control_flow(ControlFlow::Wait);
            self.start();
        }
    }

    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Menu(MenuChoice::OpenManager) => self.open_manager(None),
            UserEvent::OpenManager(page) => self.open_manager(page),
            UserEvent::Menu(MenuChoice::ToggleBoxes) => {
                let off = self.settings.desktop_mode != DesktopMode::Off;
                self.save_settings(|s| {
                    s.desktop_mode = if off {
                        DesktopMode::Off
                    } else if s.managed_consent {
                        DesktopMode::Managed
                    } else {
                        DesktopMode::Overlay
                    };
                });
            }
            UserEvent::Menu(MenuChoice::ToggleDesktop) | UserEvent::ToggleDesktop => {
                self.toggle_desktop()
            }
            UserEvent::Menu(MenuChoice::Quit) | UserEvent::Quit => self.quit(event_loop),
            UserEvent::Menu(MenuChoice::RestorePreviousLayout) => layouts::run_in_background(
                &self.store,
                &self.bus,
                "restore_previous",
                serde_json::Value::Null,
            ),
            UserEvent::Menu(MenuChoice::ApplyLayout(id)) => layouts::run_in_background(
                &self.store,
                &self.bus,
                "apply_layout",
                serde_json::json!({ "id": id }),
            ),
            UserEvent::Menu(MenuChoice::OpenLayouts) => {
                self.open_manager(Some(layouts::MODULE.into()))
            }
            UserEvent::Menu(MenuChoice::Search) => search::open(&self.bus, None),
            UserEvent::Menu(MenuChoice::OpenWidgets) => self.open_manager(Some("widgets".into())),
            UserEvent::Menu(MenuChoice::OpenWallpapers) => {
                self.open_manager(Some("wallpapers".into()))
            }
            UserEvent::Menu(MenuChoice::NextWallpaper) => {
                let _ = wallpaper::handle("next");
            }
            // Le bureau tient la boîte privée : la demande lui est relayée.
            UserEvent::Menu(MenuChoice::LockVault) => self.bus.publish(Event::Module {
                module: "vault".into(),
                action: "lock".into(),
                data: serde_json::Value::Null,
            }),
            UserEvent::LayoutsChanged => {
                self.register_hotkeys();
                self.refresh_tray();
            }
            UserEvent::ReloadSettings => self.reload_settings(),
            UserEvent::RestoreFinder => self.save_settings(|s| {
                if s.desktop_mode == DesktopMode::Managed {
                    s.desktop_mode = DesktopMode::Overlay;
                }
            }),
            UserEvent::Hotkey(action, down) => match action {
                Action::ToggleDesktop if down => self.toggle_desktop(),
                Action::MainWindow if down => self.open_manager(None),
                Action::Search if down => search::open(&self.bus, None),
                Action::Layout(id) if down => layouts::run_in_background(
                    &self.store,
                    &self.bus,
                    "apply_layout",
                    serde_json::json!({ "id": id }),
                ),
                Action::QuickAccess => match self.settings.hotkeys.quick_access_mode {
                    QuickAccessMode::Hold => {
                        self.quick_access = down;
                        self.bus.publish(Event::QuickAccess { active: down })
                    }
                    QuickAccessMode::Toggle if down => {
                        self.quick_access = !self.quick_access;
                        self.bus.publish(Event::QuickAccess {
                            active: self.quick_access,
                        })
                    }
                    QuickAccessMode::Toggle => {}
                },
                _ => {}
            },
        }
    }
}
