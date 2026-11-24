//! Cœur de Tidy : le modèle (boîtes, éléments, règles, dispositions, widgets), sa
//! persistance et les événements qui circulent entre les modules.
//!
//! Cette crate ne dépend d'aucune interface ni d'aucun OS : toute la logique y est
//! testable sans fenêtre. Les écrans sont décrits par [`screen::Screen`], fournis par
//! `tidy-platform`.

pub mod category;
pub mod events;
pub mod geometry;
pub mod icon_stash;
pub mod model;
pub mod organize;
pub mod rules;
pub mod scan;
pub mod screen;
pub mod settings;
pub mod snap;
pub mod sort;
pub mod store;

pub use model::{BoxId, DeskBox, DesktopCommand, DesktopState};
pub use settings::Settings;
pub use store::Store;

/// Nom de l'application (dossier de données, socket, journaux).
pub const APP_NAME: &str = "Tidy";
