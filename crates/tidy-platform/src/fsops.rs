//! Renommage sans écrasement : si la cible existe (créée entre la vérification et le
//! renommage, par une autre appli ou une synchronisation), l'opération échoue au lieu
//! de remplacer le fichier de l'utilisateur.
//!
//! - macOS : `renamex_np(…, RENAME_EXCL)` ;
//! - Windows : `MoveFileExW` sans `MOVEFILE_REPLACE_EXISTING` ;
//! - autres : lien dur puis retrait de l'ancien nom (fichiers), erreur pour un dossier.

use std::io;
use std::path::Path;

/// Renomme `from` en `to` sans jamais remplacer `to` : erreur
/// [`io::ErrorKind::AlreadyExists`] si `to` existe déjà. Un simple changement de
/// casse du même élément (« photo.JPG » → « photo.jpg ») est permis.
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    // Windows : MoveFileExW change la casse d'un élément sans option particulière.
    #[cfg(unix)]
    if same_item_other_case(from, to) {
        return std::fs::rename(from, to);
    }
    imp::rename(from, to)
}

/// `to` ne diffère de `from` que par la casse et désigne le même élément (système de
/// fichiers insensible à la casse) : le renommage ne remplace rien.
#[cfg(unix)]
fn same_item_other_case(from: &Path, to: &Path) -> bool {
    let (Some(a), Some(b)) = (from.file_name(), to.file_name()) else {
        return false;
    };
    if from.parent() != to.parent()
        || a == b
        || a.to_string_lossy().to_lowercase() != b.to_string_lossy().to_lowercase()
    {
        return false;
    }
    match (
        std::fs::symlink_metadata(from),
        std::fs::symlink_metadata(to),
    ) {
        (Ok(m1), Ok(m2)) => same_file(&m1, &m2),
        // La cible n'existe pas (système sensible à la casse) : renommage ordinaire.
        (Ok(_), Err(e)) => e.kind() == io::ErrorKind::NotFound,
        _ => false,
    }
}

#[cfg(unix)]
fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::{CString, c_char, c_int, c_uint};
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    /// `<stdio.h>` : échouer si la cible existe.
    const RENAME_EXCL: c_uint = 0x0000_0004;

    unsafe extern "C" {
        fn renamex_np(from: *const c_char, to: *const c_char, flags: c_uint) -> c_int;
    }

    pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
        let from = CString::new(from.as_os_str().as_bytes())?;
        let to = CString::new(to.as_os_str().as_bytes())?;
        // SAFETY: deux chaînes C valides terminées par un zéro, vivantes pendant
        // l'appel ; `renamex_np` ne les garde pas.
        let r = unsafe { renamex_np(from.as_ptr(), to.as_ptr(), RENAME_EXCL) };
        if r == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
        let (from, to) = (wide(from), wide(to));
        // Sans MOVEFILE_REPLACE_EXISTING : échec (ERROR_ALREADY_EXISTS) si la cible
        // existe.
        // SAFETY: deux chaînes UTF-16 terminées par un zéro, vivantes pendant l'appel.
        let ok = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) };
        if ok != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod imp {
    use std::io;
    use std::path::Path;

    pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
        if std::fs::symlink_metadata(from)?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "renommage sans écrasement d'un dossier : pas disponible sur cet OS",
            ));
        }
        // Le lien dur échoue si la cible existe.
        std::fs::hard_link(from, to)?;
        std::fs::remove_file(from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_replaces_an_existing_item() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let e = rename_no_replace(&a, &b).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        assert_eq!(std::fs::read(&a).unwrap(), b"a");

        let c = dir.path().join("c.txt");
        rename_no_replace(&a, &c).unwrap();
        assert!(!a.exists());
        assert_eq!(std::fs::read(&c).unwrap(), b"a");
    }

    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn folders_and_case_changes() {
        let dir = tempfile::tempdir().unwrap();
        let (d, e) = (dir.path().join("D"), dir.path().join("E"));
        std::fs::create_dir(&d).unwrap();
        std::fs::create_dir(&e).unwrap();
        assert!(rename_no_replace(&d, &e).is_err());
        let f = dir.path().join("F");
        rename_no_replace(&d, &f).unwrap();
        assert!(f.is_dir() && !d.exists());

        // Changement de casse seul : permis (même élément).
        let photo = dir.path().join("photo.JPG");
        std::fs::write(&photo, b"x").unwrap();
        let lower = dir.path().join("photo.jpg");
        rename_no_replace(&photo, &lower).unwrap();
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("photo"))
            .collect();
        assert_eq!(names, ["photo.jpg"]);
    }
}
