//! Dossier de données et fichiers JSON versionnés.
//!
//! macOS : `~/Library/Application Support/Tidy/` ; la variable `TIDY_DATA_DIR` le
//! remplace (essais avec des données jetables). L'écriture passe par un fichier
//! temporaire renommé : un plantage en pleine sauvegarde ne corrompt jamais un fichier.
//! Un fichier illisible est mis de côté (`.bad`) et remplacé par les valeurs par
//! défaut : Tidy démarre toujours.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Fichier JSON versionné, avec ses migrations.
pub trait Versioned: Serialize + DeserializeOwned + Default {
    /// Version écrite par ce code.
    const CURRENT_VERSION: u32;
    /// Nom du fichier dans le dossier de données.
    const FILE_NAME: &'static str;

    /// Fait passer `value` de la version `from` à `from + 1`. À compléter à chaque
    /// changement de format incompatible ; la version 1 n'a pas de migration.
    fn migrate_step(_value: serde_json::Value, from: u32) -> Result<serde_json::Value> {
        bail!("pas de migration depuis la version {from}")
    }
}

/// Applique les migrations jusqu'à la version courante.
pub fn migrate<T: Versioned>(mut value: serde_json::Value) -> Result<T> {
    let mut version = value
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(T::CURRENT_VERSION);
    if version > T::CURRENT_VERSION {
        bail!(
            "{} est en version {version}, plus récente que ce Tidy ({})",
            T::FILE_NAME,
            T::CURRENT_VERSION
        );
    }
    while version < T::CURRENT_VERSION {
        value = T::migrate_step(value, version)?;
        version += 1;
        if let Some(obj) = value.as_object_mut() {
            obj.insert("schema_version".into(), version.into());
        }
    }
    Ok(serde_json::from_value(value)?)
}

/// Longueur maximale d'un chemin de socket Unix, marge comprise.
const SOCKET_PATH_MAX: usize = 100;

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Dossier de l'utilisateur, ou celui de `TIDY_DATA_DIR`.
    pub fn open_default() -> Result<Self> {
        if let Some(dir) = std::env::var_os("TIDY_DATA_DIR") {
            return Self::open(Path::new(&dir));
        }
        let dirs = directories::ProjectDirs::from("", "", crate::APP_NAME)
            .context("dossier personnel introuvable")?;
        Self::open(dirs.data_dir())
    }

    /// Dossier donné (tests).
    pub fn open(root: &Path) -> Result<Self> {
        fs::create_dir_all(root).with_context(|| format!("création de {}", root.display()))?;
        Ok(Self {
            root: root.to_owned(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Socket de l'agent : dans le dossier de données, sauf si le chemin dépasse la
    /// limite des sockets Unix (104 octets sur macOS, cas d'un `TIDY_DATA_DIR` profond) ;
    /// il va alors dans le dossier temporaire, sous un nom tiré du dossier de données.
    pub fn socket_path(&self) -> PathBuf {
        let path = self.root.join("tidy.sock");
        if path.as_os_str().len() < SOCKET_PATH_MAX {
            return path;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&self.root, &mut hasher);
        let hash = std::hash::Hasher::finish(&hasher);
        std::env::temp_dir().join(format!("tidy-{hash:016x}.sock"))
    }

    /// Dossier des journaux.
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Fichier présent tant que Tidy a masqué les icônes du Finder : il contient la
    /// valeur d'origine du réglage, à rétablir.
    pub fn finder_marker_path(&self) -> PathBuf {
        self.root.join("finder-hidden.json")
    }

    pub fn path_of<T: Versioned>(&self) -> PathBuf {
        self.root.join(T::FILE_NAME)
    }

    /// Lit `T`. Absent : valeurs par défaut. Illisible ou trop récent : mis de côté en
    /// `.bad`, valeurs par défaut.
    pub fn load<T: Versioned>(&self) -> T {
        let path = self.path_of::<T>();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return T::default(),
            Err(e) => {
                log::warn!("lecture de {} impossible : {e}", path.display());
                return T::default();
            }
        };
        let parsed = serde_json::from_str::<serde_json::Value>(&text)
            .map_err(anyhow::Error::from)
            .and_then(migrate::<T>);
        match parsed {
            Ok(value) => value,
            Err(e) => {
                let bad = path.with_extension("json.bad");
                log::warn!(
                    "{} illisible ({e:#}) : mis de côté dans {}",
                    path.display(),
                    bad.display()
                );
                let _ = fs::rename(&path, &bad);
                T::default()
            }
        }
    }

    /// Enregistre `value` de façon atomique.
    pub fn save<T: Versioned>(&self, value: &T) -> Result<()> {
        let text = serde_json::to_string_pretty(value)?;
        write_atomic(&self.path_of::<T>(), text.as_bytes())
    }
}

/// Dossier du bureau de l'utilisateur, ou celui de `TIDY_DESKTOP_DIR` (essais sur un
/// faux bureau).
pub fn desktop_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("TIDY_DESKTOP_DIR") {
        return Some(PathBuf::from(dir));
    }
    directories::UserDirs::new().and_then(|d| d.desktop_dir().map(Path::to_path_buf))
}

/// Bureau public de Windows (`C:\Users\Public\Desktop`) : l'Explorateur montre ses
/// éléments (souvent les raccourcis posés par les installateurs) avec ceux de
/// l'utilisateur. `TIDY_PUBLIC_DESKTOP_DIR` le remplace (essais) ; avec un faux bureau
/// (`TIDY_DESKTOP_DIR`) et sans cette variable : aucun.
pub fn public_desktop_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("TIDY_PUBLIC_DESKTOP_DIR") {
        return Some(PathBuf::from(dir));
    }
    if !cfg!(windows) || std::env::var_os("TIDY_DESKTOP_DIR").is_some() {
        return None;
    }
    let dir = PathBuf::from(std::env::var_os("PUBLIC")?).join("Desktop");
    dir.is_dir().then_some(dir)
}

/// Dossiers dont les éléments s'affichent sur le bureau : celui de l'utilisateur
/// d'abord, puis le bureau public s'il existe.
pub fn desktop_dirs() -> Vec<PathBuf> {
    desktop_dir()
        .into_iter()
        .chain(public_desktop_dir())
        .collect()
}

/// Chemin d'un élément du bureau désigné par son nom : dans le premier dossier qui le
/// contient, sinon dans le premier dossier.
pub fn resolve_desktop_item(dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    dirs.iter()
        .map(|d| d.join(name))
        .find(|p| p.symlink_metadata().is_ok())
        .or_else(|| dirs.first().map(|d| d.join(name)))
}

/// Écrit `bytes` dans `path` via un fichier temporaire du même dossier, synchronisé
/// puis renommé.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().context("chemin sans dossier")?;
    fs::create_dir_all(dir)?;
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    {
        let mut file =
            fs::File::create(&tmp).with_context(|| format!("création de {}", tmp.display()))?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path).with_context(|| format!("écriture de {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{DesktopMode, Settings};

    #[test]
    fn missing_file_gives_defaults_and_save_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut s: Settings = store.load();
        assert_eq!(s, Settings::default());
        s.desktop_mode = DesktopMode::Off;
        store.save(&s).unwrap();
        assert_eq!(store.load::<Settings>(), s);
    }

    #[test]
    fn unreadable_file_is_set_aside() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        fs::write(store.path_of::<Settings>(), "{ pas du json").unwrap();
        assert_eq!(store.load::<Settings>(), Settings::default());
        assert!(dir.path().join("config.json.bad").exists());
    }

    #[test]
    fn long_data_dirs_get_a_short_socket() {
        let dir = tempfile::tempdir().unwrap();
        let short = Store::open(dir.path()).unwrap();
        let deep = dir.path().join("x".repeat(120));
        let long = Store::open(&deep).unwrap();
        assert!(long.socket_path().as_os_str().len() < SOCKET_PATH_MAX);
        assert_ne!(long.socket_path(), short.socket_path());
        // Stable d'un appel à l'autre.
        assert_eq!(
            long.socket_path(),
            Store::open(&deep).unwrap().socket_path()
        );
    }

    #[test]
    fn newer_version_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        fs::write(store.path_of::<Settings>(), r#"{"schema_version": 99}"#).unwrap();
        assert_eq!(store.load::<Settings>(), Settings::default());
    }
}
