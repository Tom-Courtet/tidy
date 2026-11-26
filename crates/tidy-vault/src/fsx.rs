//! Outils de fichiers du coffre : écriture atomique, parcours d'un dossier, effacement
//! des copies temporaires, noms uniques.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::crypto;

/// Écrit `bytes` dans `path` sans jamais laisser un fichier à moitié écrit : fichier
/// voisin, `fsync`, puis renommage par-dessus.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let token = crypto::random::<6>()
        .map(|b| crypto::hex(&b))
        .map_err(|e| io::Error::other(e.to_string()))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{token}.tmp"));
    let written = (|| {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written?;
    sync_dir(dir);
    Ok(())
}

/// `fsync` du dossier (Unix) : le renommage survit à une coupure de courant.
pub fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// Un élément trouvé par [`walk`].
#[derive(Debug, Clone)]
pub struct Found {
    pub path: PathBuf,
    /// Chemin relatif, composants séparés par `/`.
    pub rel: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub meta: fs::Metadata,
}

/// Tout le contenu de `dir` (sans `dir` lui-même), parents avant enfants. Les liens
/// symboliques sont signalés, jamais suivis.
pub fn walk(dir: &Path) -> io::Result<Vec<Found>> {
    let mut out = Vec::new();
    walk_into(dir, "", &mut out)?;
    Ok(out)
}

fn walk_into(dir: &Path, prefix: &str, out: &mut Vec<Found>) -> io::Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let meta = fs::symlink_metadata(e.path())?;
        let found = Found {
            path: e.path(),
            rel: rel.clone(),
            is_dir: meta.is_dir(),
            is_symlink: meta.file_type().is_symlink(),
            meta,
        };
        let recurse = found.is_dir && !found.is_symlink;
        out.push(found);
        if recurse {
            walk_into(&e.path(), &rel, out)?;
        }
    }
    Ok(())
}

/// Écrase de zéros puis supprime une copie temporaire (fichier ou dossier) créée par
/// Tidy. Rend ce qui n'a pas pu être supprimé (fichier encore ouvert par une appli…).
/// À n'appeler que sur des copies dont le contenu est déjà dans le coffre.
pub fn wipe_tree(path: &Path) -> Vec<PathBuf> {
    let mut kept = Vec::new();
    let Ok(meta) = fs::symlink_metadata(path) else {
        return kept;
    };
    if meta.is_dir() && !meta.file_type().is_symlink() {
        match walk(path) {
            Ok(items) => {
                for f in items.iter().filter(|f| !f.is_dir || f.is_symlink) {
                    if !wipe_file(&f.path, &f.meta) {
                        kept.push(f.path.clone());
                    }
                }
                // Dossiers, enfants d'abord.
                for d in items.iter().rev().filter(|f| f.is_dir && !f.is_symlink) {
                    let _ = fs::remove_dir(&d.path);
                }
            }
            Err(_) => kept.push(path.to_path_buf()),
        }
        if fs::remove_dir(path).is_err() && kept.is_empty() && path.exists() {
            kept.push(path.to_path_buf());
        }
    } else if !wipe_file(path, &meta) {
        kept.push(path.to_path_buf());
    }
    kept
}

fn wipe_file(path: &Path, meta: &fs::Metadata) -> bool {
    if meta.is_file() {
        let overwrite = (|| {
            let mut f = OpenOptions::new().write(true).open(path)?;
            let zeros = vec![0u8; 64 * 1024];
            let mut left = meta.len();
            while left > 0 {
                let n = left.min(zeros.len() as u64) as usize;
                f.write_all(&zeros[..n])?;
                left -= n as u64;
            }
            f.sync_all()
        })();
        if let Err(e) = overwrite {
            log::warn!("coffre : copie temporaire non écrasée : {e}");
            return false;
        }
    }
    match fs::remove_file(path) {
        Ok(()) => true,
        Err(e) if e.kind() == io::ErrorKind::NotFound => true,
        Err(e) => {
            log::warn!("coffre : copie temporaire non supprimée : {e}");
            false
        }
    }
}

/// Nom libre : `nom`, sinon `nom (2)`, `nom (3)`… (l'extension d'un fichier est
/// gardée à la fin).
pub fn unique_name(name: &str, is_dir: bool, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_owned();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if !is_dir && i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2..)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| name.to_owned())
}

/// Chemin libre dans `dir` pour `name`.
pub fn unique_path(dir: &Path, name: &str, is_dir: bool) -> PathBuf {
    let n = unique_name(name, is_dir, |c| fs::symlink_metadata(dir.join(c)).is_ok());
    dir.join(n)
}

/// Nom de fichier acceptable sur Windows comme sur macOS.
pub fn valid_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && trimmed == name
        && name != "."
        && name != ".."
        && !name.ends_with('.')
        && !name.chars().any(|c| {
            c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_names_keep_the_extension() {
        let taken = ["a.txt", "a (2).txt", "Dossier"];
        let t = |n: &str| taken.contains(&n);
        assert_eq!(unique_name("b.txt", false, t), "b.txt");
        assert_eq!(unique_name("a.txt", false, t), "a (3).txt");
        assert_eq!(unique_name("Dossier", true, t), "Dossier (2)");
        assert_eq!(
            unique_name(".profil", false, |n| n == ".profil"),
            ".profil (2)"
        );
    }

    #[test]
    fn names_are_checked() {
        assert!(valid_name("rapport final.pdf"));
        assert!(!valid_name(""));
        assert!(!valid_name(" a"));
        assert!(!valid_name("a/b"));
        assert!(!valid_name("a:b"));
        assert!(!valid_name(".."));
        assert!(!valid_name("fin."));
    }

    #[test]
    fn wipe_removes_copies() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("copie");
        fs::create_dir_all(root.join("sous")).unwrap();
        fs::write(root.join("a.txt"), b"secret").unwrap();
        fs::write(root.join("sous/b.txt"), b"secret").unwrap();
        assert!(wipe_tree(&root).is_empty());
        assert!(!root.exists());
        let file = dir.path().join("seul.txt");
        fs::write(&file, b"x").unwrap();
        assert!(wipe_tree(&file).is_empty());
        assert!(!file.exists());
    }
}
