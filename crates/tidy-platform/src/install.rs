//! Installation (lot K) : ce que Tidy pose dans le système quand on l'installe sous
//! Windows, sans droits d'administrateur, et retire à la désinstallation.
//!
//! Les chemins et les valeurs sont calculés par l'appli (logique pure, testée) ; ici,
//! seulement les écritures et les lectures du système. Ailleurs que sous Windows,
//! ces fonctions ne font rien (ou répondent « pas disponible »).

use std::path::{Path, PathBuf};

use anyhow::Result;

/// Entrée « Applications et fonctionnalités » de Tidy, sous `HKEY_CURRENT_USER`.
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Tidy";

/// Valeur de registre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegValue {
    /// Chaîne (`REG_SZ`).
    Text(String),
    /// Nombre (`REG_DWORD`).
    Number(u32),
}

/// Écrit l'entrée « Applications et fonctionnalités » (remplace l'ancienne).
pub fn write_uninstall_entry(values: &[(&str, RegValue)]) -> Result<()> {
    #[cfg(windows)]
    {
        crate::windows::install::write_uninstall_entry(values)
    }
    #[cfg(not(windows))]
    {
        let _ = values;
        anyhow::bail!("installation : seulement sous Windows")
    }
}

/// Retire l'entrée « Applications et fonctionnalités » (sans erreur si absente).
pub fn remove_uninstall_entry() -> Result<()> {
    #[cfg(windows)]
    {
        crate::windows::install::remove_uninstall_entry()
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

/// Valeur texte de l'entrée « Applications et fonctionnalités » (`DisplayVersion`,
/// `InstallLocation`…), si Tidy est installé.
pub fn uninstall_entry_value(name: &str) -> Option<String> {
    #[cfg(windows)]
    {
        crate::windows::install::uninstall_entry_value(name)
    }
    #[cfg(not(windows))]
    {
        let _ = name;
        None
    }
}

/// Retire l'identifiant des notifications (`AppUserModelId`) posé par les widgets.
pub fn remove_notification_id() -> Result<()> {
    #[cfg(windows)]
    {
        crate::windows::install::remove_notification_id()
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

/// Crée (ou remplace) un raccourci `.lnk` vers `target`.
pub fn create_shortcut(
    link: &Path,
    target: &Path,
    icon: Option<&Path>,
    description: &str,
) -> Result<()> {
    #[cfg(windows)]
    {
        crate::windows::install::create_shortcut(link, target, icon, description)
    }
    #[cfg(not(windows))]
    {
        let _ = (link, target, icon, description);
        anyhow::bail!("raccourcis : seulement sous Windows")
    }
}

/// Entrée « Créer une boîte » du menu du bureau, vers l'exécutable `exe` (et non
/// celui qui tourne).
pub fn set_desktop_menu_for(exe: &Path, label: &str) -> Result<()> {
    #[cfg(windows)]
    {
        crate::windows::registry::set_desktop_menu_for(exe, label)
    }
    #[cfg(not(windows))]
    {
        let _ = (exe, label);
        Ok(())
    }
}

/// Démarrage avec la session vers l'exécutable `exe` (et non celui qui tourne).
pub fn set_login_for(exe: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        crate::windows::registry::set_login_for(exe)
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
        Ok(())
    }
}

/// Processus (autres que celui-ci) lancés depuis l'un de ces exécutables.
pub fn processes_of(exes: &[PathBuf]) -> Vec<u32> {
    #[cfg(windows)]
    {
        crate::windows::install::processes_of(exes)
    }
    #[cfg(not(windows))]
    {
        unix_processes_of(exes)
    }
}

/// Liste des processus par `ps` (macOS, autres Unix) : `comm` y est le chemin de
/// l'exécutable tel qu'il a été lancé.
#[cfg(not(windows))]
fn unix_processes_of(exes: &[PathBuf]) -> Vec<u32> {
    let Ok(out) = std::process::Command::new("/bin/ps")
        .args(["-axo", "pid=,comm="])
        .output()
    else {
        return Vec::new();
    };
    parse_ps(
        &String::from_utf8_lossy(&out.stdout),
        exes,
        std::process::id(),
    )
}

/// Lignes « pid chemin » de `ps` dont le chemin est l'un des `exes`, sauf `me`.
#[cfg(not(windows))]
fn parse_ps(text: &str, exes: &[PathBuf], me: u32) -> Vec<u32> {
    text.lines()
        .filter_map(|line| {
            let (pid, path) = line.trim().split_once(' ')?;
            let pid: u32 = pid.parse().ok()?;
            let path = Path::new(path.trim());
            (pid != me && exes.iter().any(|exe| exe == path)).then_some(pid)
        })
        .collect()
}

/// Boîte de message du système (Windows), pour `--install` et `--uninstall` lancés
/// sans console. Ailleurs : rien (le texte est aussi écrit sur la sortie).
pub fn message_box(title: &str, text: &str, error: bool) {
    #[cfg(windows)]
    crate::windows::install::message_box(title, text, error);
    #[cfg(not(windows))]
    let _ = (title, text, error);
}

/// Supprime ces fichiers (puis `dir`, s'il est vide) quelques secondes après la fin
/// de ce processus : l'exécutable installé ne peut pas s'effacer lui-même tant qu'il
/// tourne (Windows).
pub fn delete_after_exit(files: &[PathBuf], dir: Option<&Path>) -> Result<()> {
    #[cfg(windows)]
    {
        crate::windows::install::delete_after_exit(files, dir)
    }
    #[cfg(not(windows))]
    {
        let _ = (files, dir);
        anyhow::bail!("suppression différée : seulement sous Windows")
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn ps_lines_match_full_paths_only() {
        let exe = PathBuf::from("/Applications/Tidy.app/Contents/MacOS/tidy");
        let text = "  12 /Applications/Tidy.app/Contents/MacOS/tidy\n\
                    13 /usr/bin/tidy\n\
                    14 /Applications/Tidy.app/Contents/MacOS/tidy\n\
                    pas une ligne\n";
        assert_eq!(parse_ps(text, std::slice::from_ref(&exe), 14), vec![12]);
        assert!(parse_ps(text, &[], 0).is_empty());
    }
}
