//! Corbeille (crate `trash`) : jamais de suppression définitive.
//!
//! - Windows : `IFileOperation` avec « autoriser l'annulation » ; un élément que la
//!   corbeille refuse (trop gros, lecteur réseau) est **ignoré et signalé**, jamais
//!   supprimé définitivement (`FOF_WANTNUKEWARNING`).
//! - macOS : `NSFileManager trashItemAtURL` (sans autorisation à demander au
//!   Finder ; « Remettre » n'est pas toujours proposé, mais le fichier est dans la
//!   corbeille).
//!
//! Tous les chemins sont vérifiés avant de commencer : un chemin relatif ou absent
//! annule tout.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Chemins acceptés : absolus et existants (un lien symbolique compte pour lui-même).
fn check(paths: &[PathBuf]) -> Result<()> {
    for path in paths {
        if !path.is_absolute() {
            bail!("corbeille : chemin relatif refusé ({})", path.display());
        }
        if is_root(path) {
            bail!("corbeille : racine refusée ({})", path.display());
        }
        std::fs::symlink_metadata(path)
            .with_context(|| format!("corbeille : {} introuvable", path.display()))?;
    }
    Ok(())
}

/// Racine d'un volume (`/`, `C:\`) : jamais à la corbeille.
fn is_root(path: &Path) -> bool {
    path.parent().is_none()
}

/// Met ces éléments à la corbeille.
pub fn move_to_trash(paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    check(paths)?;
    let paths = paths.to_vec();
    // Thread à part : sous Windows, `trash` initialise COM en mode « appartement » et
    // s'arrête net si le thread appelant l'est déjà autrement ; un thread neuf n'a pas
    // ce problème, et un arrêt net y reste confiné.
    std::thread::Builder::new()
        .name("tidy-corbeille".into())
        .spawn(move || {
            let context = context();
            context.delete_all(&paths)
        })
        .context("corbeille : thread impossible")?
        .join()
        .map_err(|_| anyhow::anyhow!("corbeille : arrêt inattendu"))?
        .context("mise à la corbeille impossible")
}

#[cfg(target_os = "macos")]
fn context() -> trash::TrashContext {
    use trash::macos::{DeleteMethod, TrashContextExtMacos};
    let mut context = trash::TrashContext::default();
    context.set_delete_method(DeleteMethod::NsFileManager);
    context
}

#[cfg(not(target_os = "macos"))]
fn context() -> trash::TrashContext {
    trash::TrashContext::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_to_do() {
        assert!(move_to_trash(&[]).is_ok());
    }

    #[test]
    fn relative_or_missing_paths_cancel_everything() {
        let dir = tempfile::tempdir().unwrap();
        let kept = dir.path().join("garde.txt");
        std::fs::write(&kept, "x").unwrap();
        let missing = dir.path().join("absent.txt");
        assert!(move_to_trash(&[kept.clone(), missing]).is_err());
        assert!(move_to_trash(&[kept.clone(), PathBuf::from("relatif.txt")]).is_err());
        // Rien n'a été touché.
        assert!(kept.exists());
    }

    #[test]
    fn roots_are_refused() {
        let root = std::env::temp_dir()
            .ancestors()
            .last()
            .unwrap()
            .to_path_buf();
        assert!(check(&[root]).is_err());
    }
}
