//! Éléments ouverts depuis le lanceur (`search-recents.json`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tidy_core::store::Versioned;

/// Un élément ouvert récemment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recent {
    pub path: PathBuf,
    /// Dernière ouverture, en secondes depuis 1970.
    pub opened_at: i64,
    /// Nombre d'ouvertures.
    #[serde(default)]
    pub count: u32,
}

/// Récents, du plus récent au plus ancien.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recents {
    pub schema_version: u32,
    pub items: Vec<Recent>,
}

impl Default for Recents {
    fn default() -> Self {
        Self {
            schema_version: Self::CURRENT_VERSION,
            items: Vec::new(),
        }
    }
}

impl Versioned for Recents {
    const CURRENT_VERSION: u32 = 1;
    const FILE_NAME: &'static str = "search-recents.json";
}

impl Recents {
    /// Nombre d'éléments gardés.
    pub const MAX: usize = 50;

    /// Note l'ouverture de `path` à l'instant `now` : il passe en tête.
    pub fn touch(&mut self, path: &Path, now: i64) {
        let count = match self.items.iter().position(|r| r.path == path) {
            Some(i) => self.items.remove(i).count,
            None => 0,
        };
        self.items.insert(
            0,
            Recent {
                path: path.to_path_buf(),
                opened_at: now,
                count: count.saturating_add(1),
            },
        );
        self.items.truncate(Self::MAX);
    }

    /// Retire `path` (mis à la corbeille).
    pub fn remove(&mut self, path: &Path) {
        self.items.retain(|r| r.path != path);
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.items.iter().map(|r| r.path.clone()).collect()
    }
}

/// Maintenant, en secondes depuis 1970.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_moves_to_front_and_counts() {
        let mut r = Recents::default();
        r.touch(Path::new("/a"), 1);
        r.touch(Path::new("/b"), 2);
        r.touch(Path::new("/a"), 3);
        assert_eq!(r.paths(), [PathBuf::from("/a"), PathBuf::from("/b")]);
        assert_eq!(r.items[0].count, 2);
        for i in 0..100 {
            r.touch(Path::new(&format!("/x{i}")), i);
        }
        assert_eq!(r.items.len(), Recents::MAX);
        r.remove(Path::new("/x99"));
        assert_eq!(r.items[0].path, PathBuf::from("/x98"));
    }

    #[test]
    fn saved_and_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let store = tidy_core::Store::open(dir.path()).unwrap();
        let mut r = Recents::default();
        r.touch(Path::new("/a"), 1);
        store.save(&r).unwrap();
        assert_eq!(store.load::<Recents>(), r);
    }
}
