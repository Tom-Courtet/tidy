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
        self.load_checked().unwrap_or_else(|e| {
            log::warn!("{e:#}");
            T::default()
        })
    }

    /// Comme [`Store::load`], mais une erreur de lecture (fichier verrouillé un instant
    /// par un antivirus ou une synchronisation, accès refusé) rend `Err` au lieu des
    /// valeurs par défaut : à utiliser avant de réécrire le fichier, pour ne jamais
    /// remplacer des données qu'on n'a pas pu lire.
    pub fn load_checked<T: Versioned>(&self) -> Result<T> {
        let path = self.path_of::<T>();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
            Err(e) => {
                return Err(e).with_context(|| format!("lecture de {} impossible", path.display()));
            }
        };
        let parsed = serde_json::from_str::<serde_json::Value>(&text)
            .map_err(anyhow::Error::from)
            .and_then(migrate::<T>);
        Ok(match parsed {
            Ok(value) => value,
            Err(e) => {
                // Une copie `.bad` plus ancienne n'est jamais écrasée.
                let mut bad = path.with_extension("json.bad");
                if bad.exists() {
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or_default();
                    bad = path.with_extension(format!("json.bad-{stamp}"));
                }
                log::warn!(
                    "{} illisible ({e:#}) : mis de côté dans {}",
                    path.display(),
                    bad.display()
                );
                let _ = fs::rename(&path, &bad);
                T::default()
            }
        })
    }

    /// Relit `T` après un changement (réglages, état…) dans `current`. Une erreur de
    /// lecture (fichier verrouillé un instant) garde `current` au lieu de le remplacer
    /// par les valeurs par défaut. `true` : relu.
    pub fn reload<T: Versioned>(&self, current: &mut T) -> bool {
        match self.load_checked() {
            Ok(value) => {
                *current = value;
                true
            }
            Err(e) => {
                log::warn!("{e:#} : valeurs précédentes gardées");
                false
            }
        }
    }

    /// Lit `T` sans jamais rien mettre de côté : absent, valeurs par défaut ; erreur de
    /// lecture (fichier verrouillé un instant…) ou contenu illisible, `Err`. Pour les
    /// fichiers qu'il serait dangereux de remplacer par des valeurs par défaut (journal
    /// des icônes masquées…).
    pub fn try_load<T: Versioned>(&self) -> Result<T> {
        let path = self.path_of::<T>();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
            Err(e) => return Err(e).with_context(|| format!("lecture de {}", path.display())),
        };
        serde_json::from_str::<serde_json::Value>(&text)
            .map_err(anyhow::Error::from)
            .and_then(migrate::<T>)
            .with_context(|| format!("{} illisible", path.display()))
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
    // Nom propre au processus et à l'écriture : deux fils qui enregistrent le même
    // fichier n'écrivent jamais dans le même fichier temporaire.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("tmp-{}-{n}", std::process::id()));
    let written = (|| -> Result<()> {
        let mut file =
            fs::File::create(&tmp).with_context(|| format!("création de {}", tmp.display()))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path).with_context(|| format!("écriture de {}", path.display()))
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{DesktopMode, Settings};

    /// Relecture impossible (ici : un dossier à la place du fichier) : les valeurs en
    /// cours sont gardées, pas remplacées par les valeurs par défaut.
    #[test]
    fn reload_keeps_current_values_on_read_errors() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let s = Settings {
            desktop_mode: DesktopMode::Off,
            ..Settings::default()
        };
        store.save(&s).unwrap();
        let mut current = Settings::default();
        assert!(store.reload(&mut current));
        assert_eq!(current, s);
        std::fs::remove_file(store.path_of::<Settings>()).unwrap();
        std::fs::create_dir(store.path_of::<Settings>()).unwrap();
        assert!(!store.reload(&mut current));
        assert_eq!(current.desktop_mode, DesktopMode::Off);
    }

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
        // Un second incident ne remplace pas la première copie.
        fs::write(store.path_of::<Settings>(), "{ encore abîmé").unwrap();
        assert_eq!(store.load::<Settings>(), Settings::default());
        assert_eq!(
            fs::read_to_string(dir.path().join("config.json.bad")).unwrap(),
            "{ pas du json"
        );
        let copies = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("config.json.bad")
            })
            .count();
        assert_eq!(copies, 2);
    }

    #[test]
    fn read_errors_are_reported_before_a_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        assert!(store.load_checked::<Settings>().is_ok(), "absent : défauts");
        // Un dossier à la place du fichier : la lecture échoue (comme un fichier
        // verrouillé).
        fs::create_dir(store.path_of::<Settings>()).unwrap();
        assert!(store.load_checked::<Settings>().is_err());
        assert_eq!(store.load::<Settings>(), Settings::default());
    }

    #[test]
    fn strict_load_never_sets_aside() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.try_load::<Settings>().unwrap(), Settings::default());
        fs::write(store.path_of::<Settings>(), "{ pas du json").unwrap();
        assert!(store.try_load::<Settings>().is_err());
        assert!(
            store.path_of::<Settings>().exists(),
            "fichier laissé en place"
        );
        assert!(!dir.path().join("config.json.bad").exists());
    }

    #[test]
    fn concurrent_saves_never_mix() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let store = store.clone();
                std::thread::spawn(move || {
                    let s = Settings {
                        desktop_mode: if i % 2 == 0 {
                            DesktopMode::Off
                        } else {
                            DesktopMode::Overlay
                        },
                        ..Settings::default()
                    };
                    for _ in 0..20 {
                        // Sous Windows, un renommage concurrent peut être refusé : seul
                        // compte le fichier final, jamais mélangé.
                        let _ = store.save(&s);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let text = fs::read_to_string(store.path_of::<Settings>()).unwrap();
        assert!(serde_json::from_str::<serde_json::Value>(&text).is_ok());
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
