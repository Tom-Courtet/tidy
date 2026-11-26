//! Portails de dossier : une boîte qui montre en direct le contenu d'un dossier situé
//! hors du bureau, avec navigation dans les sous-dossiers (retour, fil d'Ariane).
//!
//! La surveillance passe par `notify` (non récursive, sur le dossier affiché) : un
//! changement lève un drapeau et réveille la boucle, qui relit le dossier.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use notify::{RecursiveMode, Watcher};
use tidy_core::rules::ItemInfo;
use tidy_core::scan::scan_dir;

/// Surveille un dossier : `dirty` passe à `true` à chaque changement.
pub struct DirWatcher {
    _watcher: notify::RecommendedWatcher,
    dirty: Arc<AtomicBool>,
}

impl DirWatcher {
    /// Surveille `dir` (sans les sous-dossiers) ; `ctx` est réveillé à chaque
    /// changement. `None` si le système refuse.
    pub fn new(dir: &Path, ctx: &egui::Context) -> Option<Self> {
        let dirty = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&dirty);
        let ctx = ctx.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res
                && !matches!(event.kind, notify::EventKind::Access(_))
            {
                flag.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            }
        })
        .map_err(|e| log::warn!("surveillance de {} : {e}", dir.display()))
        .ok()?;
        watcher
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(|e| log::warn!("surveillance de {} : {e}", dir.display()))
            .ok()?;
        Some(Self {
            _watcher: watcher,
            dirty,
        })
    }

    /// Le dossier a changé depuis le dernier appel.
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::Relaxed)
    }
}

/// Pourquoi un dossier ne peut pas devenir un portail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortalRefusal {
    /// Le bureau lui-même : ses éléments se rangent dans les boîtes. (L'original
    /// refuse aussi les dossiers du bureau ; Tidy les accepte.)
    OnDesktop,
    NotAFolder,
}

/// Vérifie qu'un dossier peut devenir un portail : tout dossier sauf le bureau
/// lui-même. Les chemins sont comparés une fois résolus (liens, `..`).
pub fn check_folder(path: &Path, desktop: Option<&Path>) -> Result<(), PortalRefusal> {
    if !path.is_dir() {
        return Err(PortalRefusal::NotAFolder);
    }
    let resolve = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    if let Some(desktop) = desktop
        && resolve(path) == resolve(desktop)
    {
        return Err(PortalRefusal::OnDesktop);
    }
    Ok(())
}

/// État d'un portail affiché.
pub struct Portal {
    root: PathBuf,
    /// Sous-dossiers ouverts sous la racine.
    stack: Vec<String>,
    pub items: Vec<ItemInfo>,
    /// Lecture impossible (dossier absent, accès refusé).
    pub error: bool,
    watcher: Option<DirWatcher>,
    /// Compteur de versions du contenu (pour les caches de l'affichage).
    pub generation: u64,
}

impl Portal {
    pub fn new(root: PathBuf, ctx: &egui::Context) -> Self {
        let mut portal = Self {
            root,
            stack: Vec::new(),
            items: Vec::new(),
            error: false,
            watcher: None,
            generation: 0,
        };
        portal.reload(ctx);
        portal
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Dossier affiché.
    pub fn current(&self) -> PathBuf {
        let mut dir = self.root.clone();
        dir.extend(&self.stack);
        dir
    }

    /// Fil d'Ariane : nom de la racine puis des sous-dossiers ouverts.
    pub fn crumbs(&self) -> Vec<String> {
        std::iter::once(display_name(&self.root))
            .chain(self.stack.iter().cloned())
            .collect()
    }

    #[cfg(test)]
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Entre dans le sous-dossier `name` du dossier affiché.
    pub fn enter(&mut self, name: &str, ctx: &egui::Context) {
        if is_plain_name(name) && self.current().join(name).is_dir() {
            self.stack.push(name.to_owned());
            self.reload(ctx);
        }
    }

    /// Revient au niveau `depth` du fil d'Ariane (0 : la racine).
    pub fn up_to(&mut self, depth: usize, ctx: &egui::Context) {
        if depth < self.stack.len() {
            self.stack.truncate(depth);
            self.reload(ctx);
        }
    }

    /// Relit le dossier s'il a changé. Rend `true` si le contenu a été relu.
    pub fn refresh(&mut self, ctx: &egui::Context) -> bool {
        if self.watcher.as_ref().is_some_and(DirWatcher::take_dirty) {
            self.rescan();
            // Un sous-dossier affiché a pu disparaître.
            if self.error && !self.stack.is_empty() {
                self.up_to(0, ctx);
            }
            return true;
        }
        false
    }

    /// Relit le dossier affiché et le surveille.
    fn reload(&mut self, ctx: &egui::Context) {
        self.watcher = DirWatcher::new(&self.current(), ctx);
        self.rescan();
    }

    pub fn rescan(&mut self) {
        match scan_dir(&self.current()) {
            Ok(items) => {
                self.items = items;
                self.error = false;
            }
            Err(e) => {
                log::warn!("portail {} : {e}", self.current().display());
                self.items.clear();
                self.error = true;
            }
        }
        self.generation += 1;
    }
}

/// Nom affiché d'un dossier (le chemin entier pour une racine de disque).
pub fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Un nom simple (pas de séparateur, ni `..`) : on ne sort pas du portail.
fn is_plain_name(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_desktop_itself_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let desktop = dir.path().join("Bureau");
        let inside = desktop.join("Projets");
        let elsewhere = dir.path().join("Documents");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        assert_eq!(
            check_folder(&desktop, Some(&desktop)),
            Err(PortalRefusal::OnDesktop)
        );
        // Un dossier du bureau peut devenir un portail.
        assert_eq!(check_folder(&inside, Some(&desktop)), Ok(()));
        // Par un détour (`..`) aussi.
        let detour = elsewhere.join("..").join("Bureau");
        assert_eq!(
            check_folder(&detour, Some(&desktop)),
            Err(PortalRefusal::OnDesktop)
        );
        assert_eq!(check_folder(&elsewhere, Some(&desktop)), Ok(()));
        assert_eq!(
            check_folder(&dir.path().join("absent"), Some(&desktop)),
            Err(PortalRefusal::NotAFolder)
        );
    }

    #[test]
    fn navigation_stays_inside_the_portal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
        std::fs::write(dir.path().join("a/b/f.txt"), b"x").unwrap();
        let ctx = egui::Context::default();
        let mut p = Portal::new(dir.path().to_path_buf(), &ctx);
        assert_eq!(p.items.len(), 1);
        p.enter("..", &ctx);
        p.enter("a/b", &ctx);
        assert_eq!(p.depth(), 0);
        p.enter("a", &ctx);
        p.enter("b", &ctx);
        assert_eq!(p.depth(), 2);
        assert_eq!(p.items[0].name, "f.txt");
        assert_eq!(p.crumbs()[1..], ["a".to_owned(), "b".to_owned()]);
        p.up_to(1, &ctx);
        assert_eq!(p.current(), dir.path().join("a"));
        p.up_to(0, &ctx);
        assert_eq!(p.current(), dir.path());
    }
}
