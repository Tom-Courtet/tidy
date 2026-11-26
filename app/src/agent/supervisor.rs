//! Surveillance du processus du bureau (`tidy --desktop`) et sécurité du Finder.
//!
//! Règles :
//! - en mode superposé ou géré, le processus du bureau tourne ; il est relancé s'il
//!   s'arrête de lui-même ;
//! - en mode géré, les icônes du Finder ne sont masquées qu'une fois les surfaces du
//!   bureau posées ([`Command::DesktopReady`]) ;
//! - trois arrêts imprévus en une minute : Tidy abandonne, **rend les icônes du
//!   Finder** et attend un nouveau réglage ;
//! - hors du mode géré, et à l'arrêt de l'agent, les icônes du Finder sont rendues si
//!   Tidy les avait masquées (fichier témoin), y compris après un plantage précédent.

use std::collections::VecDeque;
use std::process::{Child, Command as Process};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tidy_core::Store;
use tidy_core::events::EventBus;
use tidy_core::settings::DesktopMode;
use tidy_platform::finder::{self, SystemFinder};

/// Arrêts imprévus tolérés dans [`CRASH_WINDOW`] avant d'abandonner.
const MAX_CRASHES: usize = 3;
const CRASH_WINDOW: Duration = Duration::from_secs(60);
/// Fréquence de vérification du processus.
const POLL: Duration = Duration::from_millis(500);
/// À l'arrêt de l'agent, délai laissé au bureau pour finir de lui-même (il reçoit
/// `Event::Shutdown` et verrouille la boîte privée : rechiffrement des fichiers ouverts
/// modifiés, effacement des copies en clair ; jusqu'à 10 s) avant d'être tué.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(11);

/// Action du message (module « desktop ») qui demande au bureau de s'arrêter de
/// lui-même (boîtes désactivées) : il verrouille la boîte privée puis sort.
pub const DESKTOP_QUIT: &str = "quit";

/// Message qui demande au processus du bureau de s'arrêter (voir [`DESKTOP_QUIT`]).
pub fn desktop_quit_event() -> tidy_core::events::Event {
    tidy_core::events::Event::Module {
        module: "desktop".into(),
        action: DESKTOP_QUIT.into(),
        data: serde_json::Value::Null,
    }
}

pub enum Command {
    /// Appliquer un mode (démarrage, changement de réglage).
    Apply(DesktopMode),
    DesktopReady,
    /// Arrêter le bureau, rendre le Finder, puis répondre.
    Shutdown(Sender<()>),
}

/// État partagé avec le reste de l'agent.
#[derive(Debug, Default)]
pub struct Shared {
    /// Processus du bureau lancé (il n'est peut-être pas encore abonné).
    pub desktop_running: bool,
    /// Le bureau a signalé `Request::DesktopReady` : il est abonné et applique les
    /// `Event::Desktop`. Avant, les commandes attendent dans [`Shared::pending`].
    pub desktop_ready: bool,
    /// Commandes reçues pendant le démarrage du bureau, dans l'ordre (voir
    /// [`super::organizer::route`]).
    pub pending: Vec<tidy_core::DesktopCommand>,
    /// Lancement du bureau, pour le délai de secours de [`super::organizer::flush_late`].
    pub started_at: Option<Instant>,
}

#[derive(Clone)]
pub struct Supervisor {
    tx: Sender<Command>,
}

impl Supervisor {
    pub fn spawn(store: Store, bus: EventBus, shared: Arc<Mutex<Shared>>) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("tidy-supervisor".into())
            .spawn(move || Worker::new(store, bus, shared).run(rx))
            .context("fil de surveillance du bureau")?;
        Ok(Self { tx })
    }

    pub fn send(&self, cmd: Command) {
        if self.tx.send(cmd).is_err() {
            log::warn!("surveillance du bureau arrêtée");
        }
    }

    /// Arrête le bureau et rend le Finder ; attend au plus quelques secondes.
    pub fn shutdown(&self) {
        let (tx, rx) = mpsc::channel();
        self.send(Command::Shutdown(tx));
        let _ = rx.recv_timeout(SHUTDOWN_GRACE + Duration::from_secs(4));
    }
}

struct Worker {
    store: Store,
    bus: EventBus,
    shared: Arc<Mutex<Shared>>,
    mode: DesktopMode,
    child: Option<Child>,
    crashes: VecDeque<Instant>,
    gave_up: bool,
    ready: bool,
}

impl Worker {
    fn new(store: Store, bus: EventBus, shared: Arc<Mutex<Shared>>) -> Self {
        Self {
            store,
            bus,
            shared,
            mode: DesktopMode::Off,
            child: None,
            crashes: VecDeque::new(),
            gave_up: false,
            ready: false,
        }
    }

    fn run(mut self, rx: Receiver<Command>) {
        loop {
            match rx.recv_timeout(POLL) {
                Ok(Command::Apply(mode)) => self.apply(mode),
                Ok(Command::DesktopReady) => {
                    self.ready = true;
                    if self.mode == DesktopMode::Managed {
                        self.hide_finder();
                    }
                }
                Ok(Command::Shutdown(ack)) => {
                    // `Event::Shutdown` est déjà parti : le bureau verrouille le coffre
                    // puis s'arrête de lui-même. Le tuer tout de suite laisserait les
                    // copies en clair des fichiers ouverts sur le disque.
                    self.wait_child(SHUTDOWN_GRACE);
                    self.stop_child();
                    self.restore_finder();
                    let _ = ack.send(());
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.check_child();
                    super::organizer::flush_late(&self.bus, &self.shared);
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.stop_child();
                    self.restore_finder();
                    return;
                }
            }
        }
    }

    fn apply(&mut self, mode: DesktopMode) {
        log::info!("mode du bureau : {mode:?}");
        self.mode = mode;
        self.gave_up = false;
        self.crashes.clear();
        match mode {
            DesktopMode::Off => {
                // Le bureau verrouille d'abord la boîte privée (rechiffrement des
                // fichiers ouverts modifiés, effacement des copies en clair), comme à
                // l'arrêt de l'agent ; il n'est tué qu'ensuite.
                self.quit_child();
                self.restore_finder();
                // Plus de boîtes à l'écran : les icônes rangées reviennent sur le
                // bureau du système (Windows).
                crate::icons::restore_all(&self.store);
            }
            DesktopMode::Overlay => {
                self.restore_finder();
                self.ensure_child();
            }
            DesktopMode::Managed => {
                self.ensure_child();
                if self.ready {
                    self.hide_finder();
                }
            }
        }
    }

    fn ensure_child(&mut self) {
        if self.child.is_some() {
            return;
        }
        self.ready = false;
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                log::error!("chemin de l'exécutable introuvable : {e}");
                return;
            }
        };
        match Process::new(exe).arg("--desktop").spawn() {
            Ok(child) => {
                log::info!("bureau lancé (pid {})", child.id());
                self.child = Some(child);
                self.set_running(true);
            }
            Err(e) => log::error!("lancement du bureau impossible : {e}"),
        }
    }

    /// Demande au bureau de s'arrêter de lui-même ([`DESKTOP_QUIT`]), attend au plus
    /// [`SHUTDOWN_GRACE`], puis l'arrête de force s'il tourne encore.
    fn quit_child(&mut self) {
        if self.child.is_some() {
            log::info!("arrêt du bureau demandé");
            self.bus.publish(desktop_quit_event());
            self.wait_child(SHUTDOWN_GRACE);
        }
        self.stop_child();
    }

    /// Attend au plus `limit` que le bureau s'arrête de lui-même.
    fn wait_child(&mut self, limit: Duration) {
        let Some(child) = &mut self.child else {
            return;
        };
        let start = Instant::now();
        while start.elapsed() < limit {
            match child.try_wait() {
                Ok(None) => thread::sleep(Duration::from_millis(50)),
                Ok(Some(_)) | Err(_) => return,
            }
        }
        log::warn!("le bureau ne s'est pas arrêté de lui-même : arrêt forcé");
    }

    fn stop_child(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
            log::info!("bureau arrêté");
        }
        self.ready = false;
        self.set_running(false);
    }

    /// Le bureau s'est-il arrêté de lui-même ?
    fn check_child(&mut self) {
        let Some(child) = &mut self.child else {
            return;
        };
        let status = match child.try_wait() {
            Ok(Some(status)) => status,
            Ok(None) => return,
            Err(e) => {
                log::warn!("état du bureau illisible : {e}");
                return;
            }
        };
        self.child = None;
        self.ready = false;
        self.set_running(false);
        if self.mode == DesktopMode::Off || self.gave_up {
            return;
        }
        log::warn!("le bureau s'est arrêté ({status})");
        let now = Instant::now();
        self.crashes.push_back(now);
        while self
            .crashes
            .front()
            .is_some_and(|t| now.duration_since(*t) > CRASH_WINDOW)
        {
            self.crashes.pop_front();
        }
        if self.crashes.len() >= MAX_CRASHES {
            log::error!(
                "le bureau s'est arrêté {MAX_CRASHES} fois en une minute : Tidy rend les icônes du Finder et n'insiste pas"
            );
            self.gave_up = true;
            self.restore_finder();
            crate::icons::restore_all(&self.store);
            return;
        }
        log::info!("relance du bureau");
        thread::sleep(Duration::from_millis(300 * self.crashes.len() as u64));
        self.ensure_child();
    }

    fn hide_finder(&self) {
        if let Err(e) = finder::hide_icons(&SystemFinder, &self.store.finder_marker_path()) {
            log::error!("masquage des icônes du Finder impossible : {e:#}");
        }
    }

    fn restore_finder(&self) {
        if let Err(e) = finder::restore_icons(&SystemFinder, &self.store.finder_marker_path()) {
            log::error!("icônes du Finder non rétablies : {e:#} (essayer `tidy --restore-finder`)");
        }
    }

    /// Note le lancement ou l'arrêt du bureau. À l'arrêt, les commandes qu'il n'a pas
    /// reçues (démarrage pas fini) sont appliquées au fichier.
    fn set_running(&self, running: bool) {
        if running {
            super::organizer::desktop_started(&self.shared);
        } else {
            super::organizer::desktop_stopped(&self.store, &self.bus, &self.shared);
        }
    }
}

// --- Vague 3, lot J : processus enfant générique (lecteur des fonds animés). ---

/// Processus enfant de Tidy (`tidy <arg>`) relancé s'il s'arrête de lui-même, avec la
/// règle du bureau : [`MAX_CRASHES`] arrêts imprévus en [`CRASH_WINDOW`] et Tidy
/// n'insiste plus (jusqu'au prochain [`ChildProcess::reset`]). Sert au lecteur des
/// fonds animés (`tidy --wallpaper`), lancé seulement quand un fond vidéo est actif.
pub struct ChildProcess {
    arg: &'static str,
    child: Option<Child>,
    crashes: VecDeque<Instant>,
    gave_up: bool,
}

impl ChildProcess {
    pub fn new(arg: &'static str) -> Self {
        Self {
            arg,
            child: None,
            crashes: VecDeque::new(),
            gave_up: false,
        }
    }

    pub fn running(&self) -> bool {
        self.child.is_some()
    }

    /// Tidy a renoncé après trop d'arrêts imprévus.
    pub fn gave_up(&self) -> bool {
        self.gave_up
    }

    /// Nouvelle chance après un abandon (changement de réglage).
    pub fn reset(&mut self) {
        self.gave_up = false;
        self.crashes.clear();
    }

    /// Lance le processus s'il ne tourne pas (et si Tidy n'a pas renoncé).
    pub fn ensure(&mut self) {
        if self.child.is_some() || self.gave_up {
            return;
        }
        let spawned =
            std::env::current_exe().and_then(|exe| Process::new(exe).arg(self.arg).spawn());
        match spawned {
            Ok(child) => {
                log::info!("{} lancé (pid {})", self.arg, child.id());
                self.child = Some(child);
            }
            Err(e) => log::error!("lancement de {} impossible : {e}", self.arg),
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
            log::info!("{} arrêté", self.arg);
        }
    }

    /// À appeler régulièrement : relance le processus s'il s'est arrêté de lui-même.
    /// Rend `true` s'il vient de s'arrêter.
    pub fn check(&mut self) -> bool {
        let Some(child) = &mut self.child else {
            return false;
        };
        let status = match child.try_wait() {
            Ok(Some(status)) => status,
            Ok(None) => return false,
            Err(e) => {
                log::warn!("état de {} illisible : {e}", self.arg);
                return false;
            }
        };
        self.child = None;
        log::warn!("{} s'est arrêté ({status})", self.arg);
        let now = Instant::now();
        self.crashes.push_back(now);
        while self
            .crashes
            .front()
            .is_some_and(|t| now.duration_since(*t) > CRASH_WINDOW)
        {
            self.crashes.pop_front();
        }
        if self.crashes.len() >= MAX_CRASHES {
            log::error!(
                "{} s'est arrêté {MAX_CRASHES} fois en une minute : Tidy n'insiste pas",
                self.arg
            );
            self.gave_up = true;
            return true;
        }
        thread::sleep(Duration::from_millis(300 * self.crashes.len() as u64));
        self.ensure();
        true
    }
}

impl Drop for ChildProcess {
    fn drop(&mut self) {
        self.stop();
    }
}
