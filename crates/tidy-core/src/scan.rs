//! Lecture du dossier du bureau : ce que le moteur et l'affichage savent de chaque
//! élément.
//!
//! Lot A (vague 1) : affiner la catégorie (UTI sous macOS, alias et raccourcis, liens
//! de lanceurs de jeux) sans changer la signature.

use std::path::Path;

use jiff::Timestamp;

use crate::category::classify_by_extension;
use crate::rules::ItemInfo;

/// Éléments visibles du dossier `dir` (fichiers cachés et `desktop.ini` exclus), triés
/// par nom.
pub fn scan_dir(dir: &Path) -> std::io::Result<Vec<ItemInfo>> {
    let mut items = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_hidden_name(&name) {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let is_dir = meta.is_dir();
        items.push(ItemInfo {
            category: classify_by_extension(&name, is_dir),
            size: if is_dir { 0 } else { meta.len() },
            modified: meta
                .modified()
                .ok()
                .and_then(|t| Timestamp::try_from(t).ok()),
            uti: None,
            is_dir,
            name,
        });
    }
    items.sort_by_key(|i| i.name.to_lowercase());
    Ok(items)
}

/// Fichiers système qu'un bureau n'affiche pas.
pub fn is_hidden_name(name: &str) -> bool {
    name.starts_with('.')
        || name.eq_ignore_ascii_case("desktop.ini")
        || name.eq_ignore_ascii_case("thumbs.db")
        || name == "Icon\r"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::category::Category;

    #[test]
    fn scans_a_fake_desktop() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Rapport.pdf"), b"x").unwrap();
        std::fs::write(dir.path().join(".DS_Store"), b"x").unwrap();
        std::fs::write(dir.path().join("desktop.ini"), b"x").unwrap();
        std::fs::create_dir(dir.path().join("Projets")).unwrap();
        let items = scan_dir(dir.path()).unwrap();
        let names: Vec<_> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["Projets", "Rapport.pdf"]);
        assert_eq!(items[0].category, Category::Folder);
        assert_eq!(items[1].category, Category::Document);
        assert!(items[1].modified.is_some());
    }
}
