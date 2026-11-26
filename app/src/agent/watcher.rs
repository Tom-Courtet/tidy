//! Surveillance du dossier du bureau (`tidy_core::store::desktop_dir`, qui respecte
//! `TIDY_DESKTOP_DIR`), sur un fil à part.
//!
//! Les événements du système (crate `notify`) sont regroupés (anti-rebond d'environ
//! 500 ms), puis le dossier est relu et comparé à la lecture précédente :
//! - **nouvel élément** : s'il n'est dans aucune boîte, rangé selon les règles si
//!   « ranger les nouveaux éléments » est actif, sinon ajouté aux non affectés ;
//! - **élément supprimé** : retiré des boîtes et des non affectés ;
//! - **élément renommé** : garde sa boîte et sa place. Le renommage est reconnu par les
//!   événements du système (Windows les apparie) ou, à défaut, par la même taille et la
//!   même date de modification ; un téléchargement qui se termine (`.crdownload` →
//!   nom définitif) compte comme un nouvel élément.
//!
//! Les commandes passent par [`super::organizer::send`] (relais vers le bureau), puis
//! `Event::DesktopFilesChanged` est publié. Au démarrage, les éléments arrivés pendant
//! que Tidy ne tournait pas sont traités comme nouveaux (s'il existe déjà une boîte).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use jiff::Timestamp;
use notify::event::{EventKind, ModifyKind, RenameMode};
use notify::{RecursiveMode, Watcher};
use tidy_core::events::{Event, EventBus};
use tidy_core::model::{BoxKind, ItemRef};
use tidy_core::organize;
use tidy_core::rules::MatcherRegistry;
use tidy_core::scan::{DesktopEntry, is_transient_name, scan_desktop};
use tidy_core::store::desktop_dirs;
use tidy_core::{DesktopCommand, DesktopState, Settings, Store};

use super::organizer;

/// Silence attendu avant de traiter un groupe d'événements.
const DEBOUNCE: Duration = Duration::from_millis(500);
/// Attente maximale d'un groupe (copie d'un gros dossier) avant de le traiter.
const MAX_BATCH: Duration = Duration::from_secs(3);

/// Lance la surveillance sur son propre fil.
pub fn spawn(store: Store, bus: EventBus) {
    let dirs = desktop_dirs();
    if dirs.is_empty() {
        log::warn!("surveillance du bureau : dossier du bureau introuvable");
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("surveillance-bureau".into())
        .spawn(move || {
            if let Err(e) = run(&store, &bus, &dirs) {
                log::error!("surveillance du bureau arrêtée : {e:#}");
            }
        });
    if let Err(e) = spawned {
        log::error!("surveillance du bureau impossible : {e}");
    }
}

/// `dirs` : le bureau de l'utilisateur, puis le bureau public (facultatif).
fn run(store: &Store, bus: &EventBus, dirs: &[PathBuf]) -> Result<()> {
    let dir = &dirs[0];
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .context("surveillance du bureau")?;
    watcher
        .watch(dir, RecursiveMode::NonRecursive)
        .with_context(|| format!("surveillance de {}", dir.display()))?;
    log::info!("surveillance du bureau : {}", dir.display());
    for extra in &dirs[1..] {
        match watcher.watch(extra, RecursiveMode::NonRecursive) {
            Ok(()) => log::info!("surveillance du bureau public : {}", extra.display()),
            Err(e) => log::warn!("surveillance de {} : {e}", extra.display()),
        }
    }

    let mut known = scan_desktop(dirs).with_context(|| format!("lecture de {}", dir.display()))?;
    catch_up(store, bus, &known);

    loop {
        // Premier événement d'un groupe, puis tous ceux qui suivent de près.
        let Ok(first) = rx.recv() else {
            return Ok(());
        };
        let started = Instant::now();
        let mut events = vec![first];
        loop {
            let left = MAX_BATCH.saturating_sub(started.elapsed());
            match rx.recv_timeout(DEBOUNCE.min(left)) {
                Ok(event) => events.push(event),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
        let hints = rename_hints(events.into_iter().filter_map(|e| match e {
            Ok(event) => Some(event),
            Err(e) => {
                log::debug!("surveillance du bureau : {e}");
                None
            }
        }));
        let current = match scan_desktop(dirs) {
            Ok(current) => current,
            Err(e) => {
                log::warn!("lecture de {} impossible : {e}", dir.display());
                continue;
            }
        };
        let changes = diff(&known, &current, &hints);
        if !changes.is_empty() {
            handle(store, bus, &changes);
        }
        known = current;
    }
}

/// Éléments arrivés pendant que Tidy ne tournait pas.
fn catch_up(store: &Store, bus: &EventBus, entries: &[DesktopEntry]) {
    let _organizing = organizer::organizing();
    let state: DesktopState = store.load();
    if !state.boxes.iter().any(|b| b.kind == BoxKind::Normal) {
        // Rien où ranger : le démarrage rapide s'en chargera.
        return;
    }
    let unknown: Vec<DesktopEntry> = organize::unknown_entries(&state, entries)
        .into_iter()
        .cloned()
        .collect();
    if unknown.is_empty() {
        return;
    }
    let settings: Settings = store.load();
    let commands = organize::plan_new_items(
        &state,
        &settings.organization,
        &unknown,
        &MatcherRegistry::default(),
        Timestamp::now(),
    );
    log::info!(
        "surveillance du bureau : {} élément(s) arrivé(s) en l'absence de Tidy",
        unknown.len()
    );
    if let Err(e) = organizer::send(store, bus, commands) {
        log::warn!("rangement des nouveaux éléments : {e}");
    }
}

/// Fait suivre les changements à l'état du bureau.
fn handle(store: &Store, bus: &EventBus, changes: &Changes) {
    let _organizing = organizer::organizing();
    let mut view: DesktopState = store.load();
    let settings: Settings = store.load();
    let commands = plan(&mut view, &settings, changes, Timestamp::now());
    log::debug!(
        "bureau : {} ajouté(s), {} retiré(s), {} renommé(s)",
        changes.added.len(),
        changes.removed.len(),
        changes.renamed.len()
    );
    if let Err(e) = organizer::send(store, bus, commands) {
        log::warn!("état du bureau non mis à jour : {e}");
    }
    bus.publish(Event::DesktopFilesChanged);
}

/// Commandes qui reportent `changes` sur l'état. `view` est mis à jour au fur et à
/// mesure, pour que chaque étape parte de l'état laissé par la précédente.
fn plan(
    view: &mut DesktopState,
    settings: &Settings,
    changes: &Changes,
    now: Timestamp,
) -> Vec<DesktopCommand> {
    let mut commands = Vec::new();
    let mut push = |view: &mut DesktopState, new: Vec<DesktopCommand>| {
        for command in new {
            view.apply(command.clone());
            commands.push(command);
        }
    };
    for (from, to) in &changes.renamed {
        let new = organize::plan_renamed(view, from, to);
        push(view, new);
    }
    let new = organize::plan_removed(view, &changes.removed);
    push(view, new);
    let new = organize::plan_new_items(
        view,
        &settings.organization,
        &changes.added,
        &MatcherRegistry::default(),
        now,
    );
    push(view, new);
    commands
}

/// Changements entre deux lectures du dossier.
#[derive(Debug, Default, PartialEq)]
struct Changes {
    added: Vec<DesktopEntry>,
    removed: Vec<ItemRef>,
    renamed: Vec<(ItemRef, ItemRef)>,
}

impl Changes {
    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.renamed.is_empty()
    }
}

/// Renommages annoncés par le système (ancien nom, nouveau nom).
fn rename_hints(events: impl IntoIterator<Item = notify::Event>) -> Vec<(String, String)> {
    fn name(path: &Path) -> Option<String> {
        Some(path.file_name()?.to_string_lossy().into_owned())
    }
    let mut hints = Vec::new();
    let mut pending: Option<PathBuf> = None;
    for event in events {
        match event.kind {
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if event.paths.len() == 2 => {
                if let (Some(from), Some(to)) = (name(&event.paths[0]), name(&event.paths[1])) {
                    hints.push((from, to));
                }
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
                pending = event.paths.first().cloned();
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
                if let (Some(from), Some(to)) = (pending.take(), event.paths.first())
                    && let (Some(from), Some(to)) = (name(&from), name(to))
                {
                    hints.push((from, to));
                }
            }
            _ => {}
        }
    }
    hints
}

/// Compare deux lectures du dossier.
fn diff(old: &[DesktopEntry], new: &[DesktopEntry], hints: &[(String, String)]) -> Changes {
    let old_names: BTreeSet<&str> = old.iter().map(|e| e.info.name.as_str()).collect();
    let new_names: BTreeSet<&str> = new.iter().map(|e| e.info.name.as_str()).collect();
    let mut removed: BTreeMap<&str, &DesktopEntry> = old
        .iter()
        .filter(|e| !new_names.contains(e.info.name.as_str()))
        .map(|e| (e.info.name.as_str(), e))
        .collect();
    let mut added: BTreeMap<&str, &DesktopEntry> = new
        .iter()
        .filter(|e| !old_names.contains(e.info.name.as_str()))
        .map(|e| (e.info.name.as_str(), e))
        .collect();
    let mut renamed = Vec::new();
    // Renommages annoncés.
    for (from, to) in hints {
        if removed.contains_key(from.as_str()) && added.contains_key(to.as_str()) {
            removed.remove(from.as_str());
            added.remove(to.as_str());
            renamed.push((from.clone(), to.clone()));
        }
    }
    // Sinon : même nature, même taille et même date de modification, sans ambiguïté.
    let same = |a: &DesktopEntry, b: &DesktopEntry| {
        a.info.is_dir == b.info.is_dir
            && a.info.size == b.info.size
            && a.info.modified.is_some()
            && a.info.modified == b.info.modified
    };
    let guesses: Vec<(String, String)> = removed
        .values()
        .filter_map(|r| {
            let mut candidates = added.values().filter(|a| same(r, a));
            let a = candidates.next()?;
            let unique = candidates.next().is_none()
                && removed.values().filter(|other| same(other, a)).count() == 1;
            unique.then(|| (r.info.name.clone(), a.info.name.clone()))
        })
        .collect();
    for (from, to) in guesses {
        removed.remove(from.as_str());
        added.remove(to.as_str());
        renamed.push((from, to));
    }
    // Téléchargement terminé : l'élément définitif est nouveau.
    let mut changes = Changes::default();
    for (from, to) in renamed {
        if is_transient_name(&from) {
            changes.removed.push(ItemRef(from));
            if let Some(entry) = new.iter().find(|e| e.info.name == to) {
                changes.added.push(entry.clone());
            }
        } else {
            changes.renamed.push((ItemRef(from), ItemRef(to)));
        }
    }
    changes
        .removed
        .extend(removed.keys().map(|n| ItemRef((*n).to_owned())));
    changes.added.extend(added.values().map(|e| (*e).clone()));
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidy_core::DeskBox;
    use tidy_core::category::Category;
    use tidy_core::rules::{ItemInfo, Rule};

    fn entry(name: &str, size: u64, modified: i64) -> DesktopEntry {
        DesktopEntry::new(ItemInfo {
            name: name.into(),
            is_dir: false,
            category: tidy_core::category::classify_by_extension(name, false),
            size,
            modified: Some(Timestamp::from_second(modified).unwrap()),
            uti: None,
        })
    }

    fn r(name: &str) -> ItemRef {
        ItemRef(name.into())
    }

    #[test]
    fn diff_finds_additions_removals_and_renames() {
        let old = [
            entry("a.txt", 1, 100),
            entry("b.txt", 2, 200),
            entry("c.txt", 3, 300),
            entry("d.txt", 4, 400),
        ];
        let new = [
            entry("a.txt", 1, 100),
            // Renommé (annoncé par le système).
            entry("B2.txt", 2, 200),
            // Renommé (deviné : même taille, même date).
            entry("C2.txt", 3, 300),
            // Nouveau.
            entry("e.txt", 5, 500),
        ];
        let hints = [("b.txt".into(), "B2.txt".into())];
        let changes = diff(&old, &new, &hints);
        assert_eq!(
            changes.renamed,
            [(r("b.txt"), r("B2.txt")), (r("c.txt"), r("C2.txt"))]
        );
        assert_eq!(changes.removed, [r("d.txt")]);
        assert_eq!(changes.added, [entry("e.txt", 5, 500)]);
        assert!(diff(&new, &new, &[]).is_empty());
    }

    #[test]
    fn ambiguous_guesses_and_finished_downloads_are_not_renames() {
        let old = [entry("x", 1, 100), entry("y", 1, 100)];
        let new = [entry("z", 1, 100)];
        let changes = diff(&old, &new, &[]);
        assert!(changes.renamed.is_empty());
        assert_eq!(changes.removed, [r("x"), r("y")]);
        assert_eq!(changes.added.len(), 1);

        let old = [entry("film.mp4.crdownload", 9, 100)];
        let new = [entry("film.mp4", 9, 100)];
        let changes = diff(&old, &new, &[]);
        assert!(changes.renamed.is_empty());
        assert_eq!(changes.removed, [r("film.mp4.crdownload")]);
        assert_eq!(changes.added, [entry("film.mp4", 9, 100)]);
    }

    #[test]
    fn rename_hints_pair_from_and_to() {
        let ev = |kind, paths: &[&str]| notify::Event {
            kind,
            paths: paths.iter().map(PathBuf::from).collect(),
            attrs: Default::default(),
        };
        let hints = rename_hints([
            ev(
                EventKind::Modify(ModifyKind::Name(RenameMode::From)),
                &["/bureau/a"],
            ),
            ev(
                EventKind::Modify(ModifyKind::Name(RenameMode::To)),
                &["/bureau/b"],
            ),
            ev(
                EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                &["/bureau/c", "/bureau/d"],
            ),
            ev(
                EventKind::Modify(ModifyKind::Name(RenameMode::To)),
                &["/bureau/seul"],
            ),
        ]);
        assert_eq!(hints, [("a".into(), "b".into()), ("c".into(), "d".into())]);
    }

    #[test]
    fn changes_become_commands() {
        let mut state = DesktopState::default();
        let mut docs = DeskBox::new("Documents");
        docs.tabs[0].items = vec![r("vieux.pdf"), r("garde.pdf"), r("supprimé.pdf")];
        let images = DeskBox::new("Images");
        state.rules = vec![Rule::category(images.id, Category::Image)];
        let (docs_id, images_id) = (docs.id, images.id);
        state.boxes = vec![docs, images];
        let settings = Settings::default();
        let changes = Changes {
            added: vec![entry("photo.jpg", 1, 1), entry("notes", 1, 1)],
            removed: vec![r("supprimé.pdf")],
            renamed: vec![(r("vieux.pdf"), r("neuf.pdf"))],
        };
        let mut view = state.clone();
        let commands = plan(&mut view, &settings, &changes, Timestamp::now());
        let mut applied = state.clone();
        for c in commands {
            applied.apply(c);
        }
        assert_eq!(applied, view);
        assert_eq!(
            applied.find_box(docs_id).unwrap().tabs[0].items,
            [r("neuf.pdf"), r("garde.pdf")]
        );
        assert_eq!(applied.box_of(&r("photo.jpg")), Some(images_id));
        assert_eq!(applied.unassigned, [r("notes")]);
    }

    /// Essai réel de `notify` sur un faux bureau : un fichier créé est vu.
    #[test]
    fn notify_sees_a_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        })
        .unwrap();
        watcher
            .watch(dir.path(), RecursiveMode::NonRecursive)
            .unwrap();
        // FSEvents peut mettre un instant à démarrer.
        std::thread::sleep(Duration::from_millis(200));
        std::fs::write(dir.path().join("nouveau.txt"), b"x").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = false;
        while !seen && Instant::now() < deadline {
            if let Ok(Ok(event)) = rx.recv_timeout(Duration::from_millis(200)) {
                seen = event
                    .paths
                    .iter()
                    .any(|p| p.file_name().is_some_and(|n| n == "nouveau.txt"));
            }
        }
        assert!(seen, "événement de création non reçu");
    }
}
