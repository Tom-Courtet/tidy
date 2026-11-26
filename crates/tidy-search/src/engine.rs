//! Moteur de recherche : l'index, son fil de mise à jour et son fil de requêtes.
//!
//! - **Fil d'indexation** : lit le cache (résultats immédiats), parcourt les racines,
//!   installe l'index, puis suit les changements signalés par `notify`
//!   (ReadDirectoryChangesW sous Windows, FSEvents sous macOS), regroupés par paquets.
//!   Le cache est réécrit après chaque parcours et, s'il a changé, de temps en temps.
//! - **Fil des requêtes** : la dernière requête soumise ([`Engine::submit`]) est
//!   calculée hors du fil de l'interface ; les précédentes, dépassées, sont oubliées.
//!
//! `wake` est appelé quand des résultats ou un nouvel index sont prêts (l'interface se
//! redessine).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};

use crate::cache;
use crate::config::Config;
use crate::index::Index;
use crate::query::{Hit, Query, search};

/// Attente après le dernier changement avant de mettre l'index à jour.
const DEBOUNCE: Duration = Duration::from_millis(400);
/// Au plus tard, même si les changements continuent.
const MAX_DELAY: Duration = Duration::from_secs(2);
/// Écriture du cache après des changements, au plus souvent.
const SAVE_EVERY: Duration = Duration::from_secs(60);

/// État de l'index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    /// Nombre d'éléments indexés.
    pub entries: usize,
    /// Parcours complet en cours.
    pub scanning: bool,
    /// Un index (cache ou parcours) est disponible.
    pub ready: bool,
    /// Augmente à chaque changement de l'index : les résultats affichés sont à refaire.
    pub generation: u64,
}

/// Résultats d'une requête soumise.
#[derive(Debug, Clone)]
pub struct Results {
    pub query: Query,
    pub hits: Vec<Hit>,
    pub took: Duration,
    /// Génération de l'index utilisée.
    pub generation: u64,
}

enum Command {
    Changed(Vec<PathBuf>),
    Rescan,
    Reconfigure(Config),
    SaveNow,
    Stop,
}

struct Shared {
    index: RwLock<Index>,
    status: Mutex<Status>,
    wake: Box<dyn Fn() + Send + Sync>,
    pending: Mutex<Option<Query>>,
    pending_cv: Condvar,
    results: Mutex<Option<Results>>,
    stop: AtomicBool,
    /// Interrompt le parcours en cours.
    cancel: AtomicBool,
}

impl Shared {
    fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn set_status(&self, change: impl FnOnce(&mut Status)) {
        if let Ok(mut s) = self.status.lock() {
            change(&mut s);
        }
        (self.wake)();
    }

    fn install(&self, index: Index, scanning: bool) {
        let entries = index.len();
        if let Ok(mut current) = self.index.write() {
            *current = index;
        }
        self.set_status(|s| {
            s.entries = entries;
            s.ready = true;
            s.scanning = scanning;
            s.generation += 1;
        });
    }
}

/// Moteur de recherche. Ses fils s'arrêtent quand il est libéré.
pub struct Engine {
    shared: Arc<Shared>,
    commands: Sender<Command>,
}

impl Engine {
    /// Lance l'indexation de `config`. `cache` : fichier du cache (`None` : aucun).
    pub fn start(
        config: Config,
        cache: Option<PathBuf>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            index: RwLock::new(Index::new(config.roots.clone())),
            status: Mutex::new(Status::default()),
            wake: Box::new(wake),
            pending: Mutex::new(None),
            pending_cv: Condvar::new(),
            results: Mutex::new(None),
            stop: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
        });
        let (commands, rx) = mpsc::channel();
        {
            let shared = Arc::clone(&shared);
            let tx = commands.clone();
            let spawned = std::thread::Builder::new()
                .name("tidy-index".into())
                .spawn(move || indexer(&shared, &rx, tx, config, cache));
            if let Err(e) = spawned {
                log::error!("fil d'indexation : {e}");
            }
        }
        {
            let shared = Arc::clone(&shared);
            let spawned = std::thread::Builder::new()
                .name("tidy-requêtes".into())
                .spawn(move || querier(&shared));
            if let Err(e) = spawned {
                log::error!("fil des requêtes : {e}");
            }
        }
        Self { shared, commands }
    }

    /// Soumet une requête ; ses résultats arrivent par [`Engine::take_results`].
    pub fn submit(&self, query: Query) {
        if let Ok(mut pending) = self.shared.pending.lock() {
            *pending = Some(query);
        }
        self.shared.pending_cv.notify_one();
    }

    /// Derniers résultats prêts, s'il y en a de nouveaux.
    pub fn take_results(&self) -> Option<Results> {
        self.shared.results.lock().ok()?.take()
    }

    /// Recherche tout de suite, sur ce fil.
    pub fn search_now(&self, query: &Query) -> Vec<Hit> {
        self.shared
            .index
            .read()
            .map(|index| search(&index, query))
            .unwrap_or_default()
    }

    pub fn status(&self) -> Status {
        self.shared.status()
    }

    /// Reparcourt tous les dossiers.
    pub fn rebuild(&self) {
        self.shared.cancel.store(true, Ordering::Relaxed);
        let _ = self.commands.send(Command::Rescan);
    }

    /// Change ce qu'on indexe (dossiers, exclusions) et reparcourt.
    pub fn reconfigure(&self, config: Config) {
        self.shared.cancel.store(true, Ordering::Relaxed);
        let _ = self.commands.send(Command::Reconfigure(config));
    }

    /// Écrit le cache dès que possible.
    pub fn save_cache(&self) {
        let _ = self.commands.send(Command::SaveNow);
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.cancel.store(true, Ordering::Relaxed);
        self.shared.pending_cv.notify_all();
        let _ = self.commands.send(Command::Stop);
    }
}

/// Fil des requêtes.
fn querier(shared: &Shared) {
    loop {
        let query = {
            let Ok(mut pending) = shared.pending.lock() else {
                return;
            };
            loop {
                if shared.stop.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(q) = pending.take() {
                    break q;
                }
                pending = match shared.pending_cv.wait(pending) {
                    Ok(p) => p,
                    Err(_) => return,
                };
            }
        };
        let start = Instant::now();
        let generation = shared.status().generation;
        let hits = shared
            .index
            .read()
            .map(|index| search(&index, &query))
            .unwrap_or_default();
        let took = start.elapsed();
        log::debug!(
            "recherche « {} » : {} résultats en {took:?}",
            query.text,
            hits.len()
        );
        if let Ok(mut results) = shared.results.lock() {
            *results = Some(Results {
                query,
                hits,
                took,
                generation,
            });
        }
        (shared.wake)();
    }
}

/// Surveille les racines ; les changements arrivent en commandes.
fn watch(config: &Config, tx: Sender<Command>) -> Option<notify::RecommendedWatcher> {
    let tx = Mutex::new(tx);
    let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let command = match res {
            Ok(event) if event.need_rescan() => Command::Rescan,
            Ok(event) if matches!(event.kind, notify::EventKind::Access(_)) => return,
            Ok(event) if event.paths.is_empty() => return,
            Ok(event) => Command::Changed(event.paths),
            Err(e) => {
                log::debug!("surveillance de la recherche : {e}");
                return;
            }
        };
        if let Ok(tx) = tx.lock() {
            let _ = tx.send(command);
        }
    });
    let mut watcher = match watcher {
        Ok(w) => w,
        Err(e) => {
            log::warn!("surveillance des dossiers indisponible : {e}");
            return None;
        }
    };
    for root in &config.roots {
        if let Err(e) = watcher.watch(&root.path, RecursiveMode::Recursive) {
            log::warn!("surveillance de {} impossible : {e}", root.path.display());
        }
    }
    Some(watcher)
}

/// Fil d'indexation.
fn indexer(
    shared: &Shared,
    rx: &Receiver<Command>,
    tx: Sender<Command>,
    mut config: Config,
    cache_path: Option<PathBuf>,
) {
    if let Some(path) = &cache_path {
        let start = Instant::now();
        match cache::load(path, config.signature(), config.roots.clone()) {
            Ok(index) => {
                log::info!(
                    "recherche : {} éléments lus dans le cache en {:?}",
                    index.len(),
                    start.elapsed()
                );
                shared.install(index, true);
            }
            Err(e) => log::debug!("cache de la recherche ignoré : {e:#}"),
        }
    }
    let mut _watcher = watch(&config, tx.clone());
    full_scan(shared, &config, cache_path.as_deref());

    let mut pending: HashSet<PathBuf> = HashSet::new();
    let mut first_pending: Option<Instant> = None;
    let mut dirty = false;
    let mut saved_at = Instant::now();
    loop {
        let timeout = if pending.is_empty() {
            SAVE_EVERY
        } else {
            DEBOUNCE
        };
        let command = match rx.recv_timeout(timeout) {
            Ok(c) => Some(c),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => Some(Command::Stop),
        };
        match command {
            Some(Command::Changed(paths)) => {
                pending.extend(paths);
                let first = *first_pending.get_or_insert_with(Instant::now);
                if first.elapsed() < MAX_DELAY {
                    continue;
                }
            }
            Some(Command::Rescan) => {
                pending.clear();
                first_pending = None;
                full_scan(shared, &config, cache_path.as_deref());
                dirty = false;
                saved_at = Instant::now();
                continue;
            }
            Some(Command::Reconfigure(new)) => {
                pending.clear();
                first_pending = None;
                config = new;
                _watcher = None;
                _watcher = watch(&config, tx.clone());
                full_scan(shared, &config, cache_path.as_deref());
                dirty = false;
                saved_at = Instant::now();
                continue;
            }
            Some(Command::SaveNow) => {
                if dirty {
                    save(shared, &config, cache_path.as_deref());
                    dirty = false;
                    saved_at = Instant::now();
                }
                continue;
            }
            Some(Command::Stop) => {
                if dirty {
                    save(shared, &config, cache_path.as_deref());
                }
                return;
            }
            None => {}
        }
        if !pending.is_empty() {
            apply(shared, &config, pending.drain());
            first_pending = None;
            dirty = true;
        }
        if dirty && saved_at.elapsed() >= SAVE_EVERY {
            save(shared, &config, cache_path.as_deref());
            dirty = false;
            saved_at = Instant::now();
        }
    }
}

/// Met l'index à jour pour ces chemins changés.
fn apply(shared: &Shared, config: &Config, paths: impl Iterator<Item = PathBuf>) {
    let start = Instant::now();
    let mut count = 0;
    let entries = {
        let Ok(mut index) = shared.index.write() else {
            return;
        };
        // Un dossier relu une fois par lot, même pour de nombreux fichiers changés.
        count += index.refresh_all(paths, config);
        index.len()
    };
    log::debug!(
        "recherche : {count} changements appliqués en {:?}",
        start.elapsed()
    );
    shared.set_status(|s| {
        s.entries = entries;
        s.generation += 1;
    });
}

fn full_scan(shared: &Shared, config: &Config, cache_path: Option<&Path>) {
    shared.cancel.store(false, Ordering::Relaxed);
    shared.set_status(|s| s.scanning = true);
    let start = Instant::now();
    let index = Index::scan(config, &shared.cancel);
    if shared.cancel.load(Ordering::Relaxed) {
        log::info!("recherche : parcours interrompu");
        shared.set_status(|s| s.scanning = false);
        return;
    }
    log::info!(
        "recherche : {} éléments indexés en {:?} ({} dossiers racines)",
        index.len(),
        start.elapsed(),
        config.roots.len()
    );
    write_cache(&index, config, cache_path);
    shared.install(index, false);
}

fn save(shared: &Shared, config: &Config, cache_path: Option<&Path>) {
    if let Ok(index) = shared.index.read() {
        write_cache(&index, config, cache_path);
    }
}

fn write_cache(index: &Index, config: &Config, cache_path: Option<&Path>) {
    let Some(path) = cache_path else { return };
    if let Err(e) = cache::save(index, config.signature(), path) {
        log::warn!("cache de la recherche non écrit : {e:#}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::tests::config_for;

    fn wait_for(engine: &Engine, what: impl Fn(&Engine) -> bool) {
        let start = Instant::now();
        while !what(engine) {
            assert!(start.elapsed() < Duration::from_secs(10), "délai dépassé");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn query(text: &str) -> Query {
        Query {
            text: text.into(),
            limit: 10,
            ..Query::default()
        }
    }

    #[test]
    fn indexes_caches_and_follows_changes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("racine");
        std::fs::create_dir_all(root.join("sous")).unwrap();
        std::fs::write(root.join("sous/facture mars.pdf"), "x").unwrap();
        let cache = dir.path().join("search/index.bin");
        let config = config_for(&root, false);

        let engine = Engine::start(config.clone(), Some(cache.clone()), || {});
        wait_for(&engine, |e| e.status().ready && !e.status().scanning);
        assert_eq!(engine.search_now(&query("facture")).len(), 1);
        assert!(cache.exists(), "cache écrit après le parcours");

        // Requête asynchrone.
        engine.submit(query("fact"));
        let start = Instant::now();
        let results = loop {
            if let Some(r) = engine.take_results() {
                break r;
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(results.hits.len(), 1);

        // Un nouveau fichier arrive par la surveillance (ou, à défaut, par un
        // nouveau parcours).
        std::fs::write(root.join("sous/facture avril.pdf"), "x").unwrap();
        let start = Instant::now();
        while engine.search_now(&query("facture")).len() < 2 {
            if start.elapsed() > Duration::from_secs(4) {
                engine.rebuild();
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "changement non vu"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        drop(engine);

        // Le cache donne les résultats avant la fin du parcours.
        let index = cache::load(&cache, config.signature(), config.roots.clone()).unwrap();
        assert!(!index.is_empty());
    }
}
