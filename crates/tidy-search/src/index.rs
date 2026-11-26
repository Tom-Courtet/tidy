//! Index en mémoire : la liste des fichiers et dossiers des racines.
//!
//! Compact : chaque élément garde son nom, l'identifiant de son dossier parent (les
//! chemins des dossiers sont stockés une fois), sa catégorie et sa date de
//! modification. Les noms et chemins non UTF-8 (rares) sont ignorés.
//!
//! Mise à jour : [`Index::refresh`] relit le dossier touché par un changement (et
//! parcourt les nouveaux sous-dossiers, oublie ceux qui ont disparu).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use tidy_core::category::{Category, classify_by_extension};

use crate::config::{Config, Root, is_package};

/// Au-delà, l'indexation s'arrête (protection de la mémoire).
pub const MAX_ENTRIES: usize = 2_000_000;

pub(crate) type DirId = u32;

/// Dossier sur le bureau (onglet « Bureau »).
pub(crate) const FLAG_DESKTOP: u8 = 1;
/// Dossier de programmes.
pub(crate) const FLAG_PROGRAMS: u8 = 2;

#[derive(Debug, Clone)]
pub(crate) struct Dir {
    pub path: Box<str>,
    pub flags: u8,
    /// Faux quand le dossier a disparu (l'identifiant n'est pas réutilisé).
    pub alive: bool,
}

/// Un fichier ou un dossier indexé.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub name: Box<str>,
    pub dir: DirId,
    pub category: Category,
    /// Vrai dossier (un paquet `.app` n'en est pas un ici).
    pub is_dir: bool,
    /// Date de modification, en secondes depuis 1970.
    pub modified: i64,
    /// Nom sans accents ni majuscules, s'il n'est pas en ASCII : la recherche floue est
    /// bien plus rapide sur de l'ASCII.
    pub folded: Option<Box<str>>,
}

impl Entry {
    pub fn new(name: &str, dir: DirId, category: Category, is_dir: bool, modified: i64) -> Self {
        Self {
            name: name.into(),
            dir,
            category,
            is_dir,
            modified,
            folded: (!name.is_ascii()).then(|| crate::query::fold(name).into()),
        }
    }

    /// Texte comparé à la saisie.
    pub fn key(&self) -> &str {
        self.folded.as_deref().unwrap_or(&self.name)
    }
}

/// Index des racines d'une [`Config`].
#[derive(Debug, Clone, Default)]
pub struct Index {
    pub(crate) roots: Vec<Root>,
    pub(crate) dirs: Vec<Dir>,
    pub(crate) dir_ids: HashMap<Box<str>, DirId>,
    pub(crate) entries: Vec<Entry>,
}

impl Index {
    /// Index vide pour ces racines.
    pub fn new(roots: Vec<Root>) -> Self {
        Self {
            roots,
            ..Self::default()
        }
    }

    /// Parcourt toutes les racines. `cancel` interrompt le parcours (index partiel).
    pub fn scan(config: &Config, cancel: &AtomicBool) -> Self {
        let mut index = Self::new(config.roots.clone());
        for root in &config.roots {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            index.walk(&root.path, config, cancel);
        }
        index
    }

    /// Nombre d'éléments indexés.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn path_of(&self, entry: &Entry) -> PathBuf {
        Path::new(&*self.dirs[entry.dir as usize].path).join(&*entry.name)
    }

    pub(crate) fn on_desktop(&self, entry: &Entry) -> bool {
        self.dirs[entry.dir as usize].flags & FLAG_DESKTOP != 0
    }

    /// Identifiant du dossier `path` s'il est indexé.
    pub(crate) fn dir_id(&self, path: &Path) -> Option<DirId> {
        self.dir_ids.get(path.to_str()?).copied()
    }

    fn flags_for(&self, path: &Path) -> u8 {
        self.roots
            .iter()
            .filter(|r| path.starts_with(&r.path))
            .fold(0, |f, r| {
                f | if r.desktop { FLAG_DESKTOP } else { 0 }
                    | if r.programs { FLAG_PROGRAMS } else { 0 }
            })
    }

    fn add_dir(&mut self, path: &str) -> DirId {
        if let Some(&id) = self.dir_ids.get(path) {
            return id;
        }
        let id = self.dirs.len() as DirId;
        let flags = self.flags_for(Path::new(path));
        self.dirs.push(Dir {
            path: path.into(),
            flags,
            alive: true,
        });
        self.dir_ids.insert(path.into(), id);
        id
    }

    /// Parcourt `start` et tout ce qui est dessous (sauf dossiers déjà indexés : racines
    /// imbriquées).
    fn walk(&mut self, start: &Path, config: &Config, cancel: &AtomicBool) {
        let mut stack = vec![start.to_path_buf()];
        while let Some(dir) = stack.pop() {
            if cancel.load(Ordering::Relaxed) || self.entries.len() >= MAX_ENTRIES {
                if self.entries.len() >= MAX_ENTRIES {
                    log::warn!("recherche : plus de {MAX_ENTRIES} éléments, index tronqué");
                }
                return;
            }
            let Some(text) = dir.to_str() else { continue };
            if self.dir_ids.contains_key(text) {
                continue;
            }
            let id = self.add_dir(text);
            self.list(id, &dir, config, &mut stack);
        }
    }

    /// Ajoute les éléments de `dir` ; les sous-dossiers à parcourir vont dans `subdirs`.
    fn list(&mut self, id: DirId, dir: &Path, config: &Config, subdirs: &mut Vec<PathBuf>) {
        let Ok(read) = fs::read_dir(dir) else { return };
        let programs = self.dirs[id as usize].flags & FLAG_PROGRAMS != 0;
        for item in read.flatten() {
            let file_name = item.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            if config.exclusions.excludes_name(name) {
                continue;
            }
            let Ok(file_type) = item.file_type() else {
                continue;
            };
            let Ok(meta) = item.metadata() else { continue };
            if config.exclusions.excludes_metadata(&meta) {
                continue;
            }
            let path = item.path();
            if config.exclusions.excludes_path(&path) {
                continue;
            }
            let raw_dir = file_type.is_dir();
            let is_dir = raw_dir && !is_package(name);
            let mut category = classify_by_extension(name, raw_dir);
            if programs && matches!(category, Category::WebLink | Category::Executable) {
                // Menu Démarrer : liens de jeux (Steam…) et programmes.
                category = Category::Application;
            }
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs() as i64);
            self.entries
                .push(Entry::new(name, id, category, is_dir, modified));
            if is_dir {
                subdirs.push(path);
            }
        }
    }

    /// Met l'index à jour après un changement signalé sur `changed` (créé, supprimé,
    /// renommé, modifié).
    pub fn refresh(&mut self, changed: &Path, config: &Config) {
        let Some(root) = config.root_of(changed) else {
            return;
        };
        let root_path = root.path.clone();
        if let Ok(rel) = changed.strip_prefix(&root_path) {
            let excluded = rel.components().any(|c| {
                c.as_os_str()
                    .to_str()
                    .is_none_or(|n| config.exclusions.excludes_name(n))
            });
            if excluded || config.exclusions.excludes_path(changed) {
                return;
            }
        }
        if changed == root_path {
            if root_path.is_dir() {
                let never = AtomicBool::new(false);
                self.walk(&root_path, config, &never);
            } else {
                self.remove_subtree(&root_path);
            }
            return;
        }
        // Dossier connu le plus proche au-dessus de l'élément.
        let mut target = changed.parent();
        while let Some(dir) = target {
            if let Some(id) = self.dir_id(dir) {
                self.relist(id, config);
                return;
            }
            if dir == root_path {
                break;
            }
            target = dir.parent();
        }
        // Racine encore jamais vue (créée après le démarrage).
        let never = AtomicBool::new(false);
        self.walk(&root_path, config, &never);
    }

    /// Relit le dossier `id` : ses éléments sont remplacés, les nouveaux sous-dossiers
    /// parcourus, ceux qui ont disparu oubliés.
    fn relist(&mut self, id: DirId, config: &Config) {
        let dir = PathBuf::from(&*self.dirs[id as usize].path);
        if !dir.is_dir() {
            self.remove_subtree(&dir);
            return;
        }
        let old_subdirs: Vec<Box<str>> = self
            .entries
            .iter()
            .filter(|e| e.dir == id && e.is_dir)
            .map(|e| e.name.clone())
            .collect();
        self.entries.retain(|e| e.dir != id);
        let mut subdirs = Vec::new();
        self.list(id, &dir, config, &mut subdirs);
        let present: HashSet<&str> = self
            .entries
            .iter()
            .filter(|e| e.dir == id && e.is_dir)
            .map(|e| &*e.name)
            .collect();
        let gone: Vec<PathBuf> = old_subdirs
            .iter()
            .filter(|n| !present.contains(&***n))
            .map(|n| dir.join(&**n))
            .collect();
        for path in gone {
            self.remove_subtree(&path);
        }
        let never = AtomicBool::new(false);
        for sub in subdirs {
            if self.dir_id(&sub).is_none() {
                self.walk(&sub, config, &never);
            }
        }
    }

    /// Oublie le dossier `path` et tout ce qui est dessous.
    fn remove_subtree(&mut self, path: &Path) {
        let mut dead = vec![false; self.dirs.len()];
        let mut any = false;
        for dir in self.dirs.iter_mut().filter(|d| d.alive) {
            if Path::new(&*dir.path).starts_with(path) {
                dir.alive = false;
                any = true;
            }
        }
        if !any {
            return;
        }
        for (i, dir) in self.dirs.iter().enumerate() {
            dead[i] = !dir.alive;
        }
        self.dir_ids.retain(|_, id| !dead[*id as usize]);
        self.entries.retain(|e| !dead[e.dir as usize]);
        // L'élément du dossier lui-même, dans son parent.
        if let (Some(parent), Some(name)) = (path.parent(), path.file_name())
            && let (Some(pid), Some(name)) = (self.dir_id(parent), name.to_str())
        {
            self.entries.retain(|e| !(e.dir == pid && &*e.name == name));
        }
    }

    /// Ajoute un élément sans toucher au disque (essais, banc d'essai).
    #[doc(hidden)]
    pub fn push_synthetic(&mut self, dir: &str, name: &str, is_dir: bool, modified: i64) {
        let id = self.add_dir(dir);
        self.entries.push(Entry::new(
            name,
            id,
            classify_by_extension(name, is_dir),
            is_dir,
            modified,
        ));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::config::Exclusions;

    pub fn config_for(root: &Path, desktop: bool) -> Config {
        Config {
            roots: vec![Root {
                path: root.to_path_buf(),
                desktop,
                programs: false,
            }],
            exclusions: Exclusions::new(&[], false),
        }
    }

    fn names(index: &Index) -> Vec<String> {
        let mut out: Vec<String> = index
            .entries
            .iter()
            .map(|e| {
                index
                    .path_of(e)
                    .strip_prefix(&index.roots[0].path)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        out.sort();
        out
    }

    #[test]
    fn scan_skips_exclusions_and_packages() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("Projets/node_modules/lib")).unwrap();
        fs::create_dir_all(root.join("Safari.app/Contents")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();
        fs::write(root.join("Projets/rapport.pdf"), "x").unwrap();
        fs::write(root.join("Projets/node_modules/lib/a.js"), "x").unwrap();
        fs::write(root.join("Safari.app/Contents/Info.plist"), "x").unwrap();
        fs::write(root.join(".zshrc"), "x").unwrap();
        let config = config_for(root, true);
        let index = Index::scan(&config, &AtomicBool::new(false));
        assert_eq!(
            names(&index),
            ["Projets", "Projets/rapport.pdf", "Safari.app"]
        );
        let app = index.entries.iter().find(|e| &*e.name == "Safari.app");
        assert_eq!(app.unwrap().category, Category::Application);
        assert!(index.entries.iter().all(|e| index.on_desktop(e)));
    }

    #[test]
    fn refresh_follows_changes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("A")).unwrap();
        fs::write(root.join("A/un.txt"), "x").unwrap();
        let config = config_for(root, false);
        let mut index = Index::scan(&config, &AtomicBool::new(false));
        assert_eq!(names(&index), ["A", "A/un.txt"]);

        // Nouveau fichier, nouveau dossier avec contenu.
        fs::write(root.join("A/deux.txt"), "x").unwrap();
        index.refresh(&root.join("A/deux.txt"), &config);
        fs::create_dir_all(root.join("A/B/C")).unwrap();
        fs::write(root.join("A/B/C/trois.txt"), "x").unwrap();
        index.refresh(&root.join("A/B/C/trois.txt"), &config);
        assert_eq!(
            names(&index),
            [
                "A",
                "A/B",
                "A/B/C",
                "A/B/C/trois.txt",
                "A/deux.txt",
                "A/un.txt"
            ]
        );

        // Dossier supprimé : tout son contenu disparaît.
        fs::remove_dir_all(root.join("A/B")).unwrap();
        index.refresh(&root.join("A/B"), &config);
        assert_eq!(names(&index), ["A", "A/deux.txt", "A/un.txt"]);

        // Renommage.
        fs::rename(root.join("A"), root.join("Z")).unwrap();
        index.refresh(&root.join("A"), &config);
        index.refresh(&root.join("Z"), &config);
        assert_eq!(names(&index), ["Z", "Z/deux.txt", "Z/un.txt"]);

        // Changement dans un dossier exclu : ignoré.
        fs::create_dir_all(root.join("Z/node_modules")).unwrap();
        index.refresh(&root.join("Z/node_modules/x.js"), &config);
        assert_eq!(names(&index), ["Z", "Z/deux.txt", "Z/un.txt"]);
    }
}
