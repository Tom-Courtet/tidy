//! Recherche rapide de Tidy (lot G) : index en mémoire des fichiers et dossiers de
//! dossiers réglables (bureau, Documents, Téléchargements, Images, Musique, Vidéos,
//! programmes), recherche floue insensible à la casse et aux accents, onglets,
//! récents, cache sur disque et mise à jour en direct.
//!
//! - [`Config`] : ce qu'on indexe, d'après [`tidy_core::settings::search::SearchSettings`].
//! - [`Index`] : la liste compacte des éléments ; [`search`] y cherche.
//! - [`Engine`] : index tenu à jour sur ses fils (parcours, `notify`, cache) et
//!   requêtes calculées hors du fil de l'interface.
//! - [`Recents`] : éléments ouverts depuis le lanceur (`search-recents.json`).
//! - [`spotlight`] : « Plus de résultats » sous macOS.
//!
//! Aucune interface ici : le lanceur est dans `app/src/search`.

mod cache;
mod config;
mod engine;
mod index;
mod query;
mod recents;
pub mod spotlight;
mod tab;

pub use config::{BUILTIN_EXCLUSIONS, Config, DEFAULT_KEYS, Exclusions, Root, default_folders};
pub use engine::{Engine, Results, Status};
pub use index::{Index, MAX_ENTRIES};
pub use query::{Hit, Query, display_name, normalize, search};
pub use recents::{Recent, Recents, now};
pub use tab::Tab;

/// Fichier du cache de l'index, dans le dossier de données.
pub fn cache_path(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join("search").join("index.bin")
}

/// Dossier personnel de l'utilisateur.
pub fn home_dir() -> Option<std::path::PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
}
