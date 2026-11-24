//! Icônes du bureau du Finder : les masquer (mode géré) et, surtout, les rendre.
//!
//! macOS : réglage `CreateDesktop` du domaine `com.apple.finder`, puis relance du
//! Finder. Avant de masquer, Tidy écrit un **fichier témoin** avec la valeur d'origine
//! du réglage. Tant que ce fichier existe, Tidy sait qu'il doit rendre les icônes :
//! en quittant, en désactivant les boîtes, après un plantage (au démarrage suivant),
//! ou avec `tidy --restore-finder`. Le réglage d'origine est rétabli tel quel (clé
//! absente comprise), jamais une valeur supposée.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Contenu du fichier témoin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    /// Valeur de `CreateDesktop` avant Tidy (`None` : clé absente).
    pub original: Option<bool>,
}

/// Accès au réglage du Finder (remplaçable dans les tests).
pub trait FinderPrefs {
    /// Valeur de `CreateDesktop` (`None` : clé absente).
    fn read(&self) -> Result<Option<bool>>;
    /// Écrit la valeur, ou supprime la clé (`None`).
    fn write(&self, value: Option<bool>) -> Result<()>;
    /// Relance le Finder pour appliquer le réglage.
    fn relaunch(&self) -> Result<()>;
}

/// Le vrai réglage, via `defaults` et `killall` (macOS).
pub struct SystemFinder;

#[cfg(target_os = "macos")]
impl FinderPrefs for SystemFinder {
    fn read(&self) -> Result<Option<bool>> {
        let out = std::process::Command::new("/usr/bin/defaults")
            .args(["read", "com.apple.finder", "CreateDesktop"])
            .output()
            .context("lancement de defaults")?;
        if !out.status.success() {
            // Clé absente : `defaults` échoue avec « does not exist ».
            return Ok(None);
        }
        let text = String::from_utf8_lossy(&out.stdout).trim().to_ascii_lowercase();
        Ok(Some(matches!(text.as_str(), "1" | "true" | "yes")))
    }

    fn write(&self, value: Option<bool>) -> Result<()> {
        let mut cmd = std::process::Command::new("/usr/bin/defaults");
        match value {
            Some(v) => cmd.args([
                "write",
                "com.apple.finder",
                "CreateDesktop",
                "-bool",
                if v { "true" } else { "false" },
            ]),
            None => cmd.args(["delete", "com.apple.finder", "CreateDesktop"]),
        };
        let status = cmd.status().context("lancement de defaults")?;
        anyhow::ensure!(status.success(), "defaults a échoué ({status})");
        Ok(())
    }

    fn relaunch(&self) -> Result<()> {
        // Le Finder est relancé automatiquement par launchd.
        let status = std::process::Command::new("/usr/bin/killall")
            .arg("Finder")
            .status()
            .context("lancement de killall")?;
        anyhow::ensure!(status.success(), "relance du Finder impossible ({status})");
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
impl FinderPrefs for SystemFinder {
    fn read(&self) -> Result<Option<bool>> {
        Ok(None)
    }
    fn write(&self, _: Option<bool>) -> Result<()> {
        anyhow::bail!("masquage des icônes pas encore disponible sur cet OS")
    }
    fn relaunch(&self) -> Result<()> {
        Ok(())
    }
}

/// Les icônes du Finder sont-elles masquées par Tidy (fichier témoin présent) ?
pub fn hidden_by_tidy(marker: &Path) -> bool {
    marker.exists()
}

/// Masque les icônes du Finder. Ne fait rien si Tidy les a déjà masquées.
pub fn hide_icons(prefs: &dyn FinderPrefs, marker: &Path) -> Result<()> {
    if marker.exists() {
        return Ok(());
    }
    let original = prefs.read()?;
    // Le témoin d'abord : si la suite plante, on saura quoi rétablir.
    let text = serde_json::to_string(&Marker { original })?;
    tidy_core::store::write_atomic(marker, text.as_bytes())?;
    if let Err(e) = prefs.write(Some(false)).and_then(|()| prefs.relaunch()) {
        // Échec : on remet tout comme avant.
        let _ = prefs.write(original);
        let _ = std::fs::remove_file(marker);
        return Err(e);
    }
    log::info!("icônes du Finder masquées (valeur d'origine : {original:?})");
    Ok(())
}

/// Rend les icônes du Finder si Tidy les avait masquées. `true` si quelque chose a été
/// rétabli.
pub fn restore_icons(prefs: &dyn FinderPrefs, marker: &Path) -> Result<bool> {
    let text = match std::fs::read_to_string(marker) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e).context("lecture du témoin du Finder"),
    };
    // Témoin illisible : on rend les icônes (clé supprimée = réglage par défaut).
    let original = serde_json::from_str::<Marker>(&text)
        .map(|m| m.original)
        .unwrap_or(None);
    // Ne jamais rétablir « masqué » : si l'utilisateur les avait déjà masquées avant
    // Tidy, on garde son choix ; sinon on rend les icônes.
    prefs.write(original)?;
    prefs.relaunch()?;
    std::fs::remove_file(marker).context("suppression du témoin du Finder")?;
    log::info!("icônes du Finder rétablies (valeur : {original:?})");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[derive(Default)]
    struct Fake {
        value: RefCell<Option<bool>>,
        relaunches: RefCell<u32>,
        fail_write: bool,
    }

    impl FinderPrefs for Fake {
        fn read(&self) -> Result<Option<bool>> {
            Ok(*self.value.borrow())
        }
        fn write(&self, value: Option<bool>) -> Result<()> {
            if self.fail_write && value == Some(false) {
                anyhow::bail!("refusé");
            }
            *self.value.borrow_mut() = value;
            Ok(())
        }
        fn relaunch(&self) -> Result<()> {
            *self.relaunches.borrow_mut() += 1;
            Ok(())
        }
    }

    #[test]
    fn hide_then_restore_brings_back_the_original_value() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("finder-hidden.json");
        let fake = Fake::default();
        hide_icons(&fake, &marker).unwrap();
        assert_eq!(*fake.value.borrow(), Some(false));
        assert!(hidden_by_tidy(&marker));
        // Deuxième appel : rien ne change, l'original reste « clé absente ».
        hide_icons(&fake, &marker).unwrap();
        assert!(restore_icons(&fake, &marker).unwrap());
        assert_eq!(*fake.value.borrow(), None);
        assert!(!marker.exists());
        assert_eq!(*fake.relaunches.borrow(), 2);
        // Plus rien à rétablir.
        assert!(!restore_icons(&fake, &marker).unwrap());
    }

    #[test]
    fn failed_hide_leaves_no_marker() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("finder-hidden.json");
        let fake = Fake {
            value: RefCell::new(Some(true)),
            fail_write: true,
            ..Fake::default()
        };
        assert!(hide_icons(&fake, &marker).is_err());
        assert!(!marker.exists());
        assert_eq!(*fake.value.borrow(), Some(true));
    }

    #[test]
    fn unreadable_marker_still_restores() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("finder-hidden.json");
        std::fs::write(&marker, "n'importe quoi").unwrap();
        let fake = Fake {
            value: RefCell::new(Some(false)),
            ..Fake::default()
        };
        assert!(restore_icons(&fake, &marker).unwrap());
        assert_eq!(*fake.value.borrow(), None);
    }
}
