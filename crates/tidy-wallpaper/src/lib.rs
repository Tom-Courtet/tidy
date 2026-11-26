//! Fonds d'écran de Tidy (lot J).
//!
//! - [`library`] : bibliothèque locale (`wallpapers.json`) : import de fichiers et de
//!   dossiers, catégories, favoris ;
//! - [`thumbs`] : miniatures en cache et images préparées pour le système ;
//! - [`playlist`] : ordre, aléatoire sans répétition, intervalle ;
//! - [`pause`] : règles de la pause intelligente ;
//! - [`source`] : sources en ligne branchables ([`source::WallpaperSource`]),
//!   désactivées par défaut ;
//! - [`player`] : lecteur vidéo natif (Media Foundation, AVFoundation), seul module où
//!   `unsafe` est permis.
//!
//! Les fonds fixes passent par l'API du système (`tidy-platform`), sans fenêtre ni
//! mémoire ; seules les vidéos demandent un processus (`tidy --wallpaper`).

pub mod library;
pub mod pause;
#[allow(unsafe_code)]
pub mod player;
pub mod playlist;
pub mod source;
pub mod thumbs;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use library::{Library, Wallpaper};

/// Nature d'un fond.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Image,
    Video,
}

/// Extensions d'images acceptées. Le WebP est converti en PNG pour le système (voir
/// [`thumbs::system_image`]).
pub const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "bmp", "webp"];

/// Extensions de vidéos que le lecteur du système sait lire.
#[cfg(windows)]
pub const VIDEO_EXTENSIONS: &[&str] = &["mp4", "m4v", "mov", "webm", "wmv"];
/// AVFoundation ne lit pas le WebM.
#[cfg(not(windows))]
pub const VIDEO_EXTENSIONS: &[&str] = &["mp4", "m4v", "mov"];

impl MediaKind {
    /// Nature d'un fichier d'après son extension ; `None` : pas un fond accepté.
    pub fn of(path: &Path) -> Option<MediaKind> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
            Some(MediaKind::Image)
        } else if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
            Some(MediaKind::Video)
        } else {
            None
        }
    }
}

/// Dossier des fichiers propres aux fonds d'écran (miniatures, images préparées,
/// téléchargements) dans le dossier de données. Tidy peut y supprimer ses fichiers ;
/// jamais ailleurs.
pub fn cache_dir(data_root: &Path) -> PathBuf {
    data_root.join("wallpapers")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_extension() {
        assert_eq!(MediaKind::of(Path::new("a/B.JPG")), Some(MediaKind::Image));
        assert_eq!(MediaKind::of(Path::new("x.webp")), Some(MediaKind::Image));
        assert_eq!(MediaKind::of(Path::new("x.mp4")), Some(MediaKind::Video));
        assert_eq!(MediaKind::of(Path::new("x.MOV")), Some(MediaKind::Video));
        assert_eq!(MediaKind::of(Path::new("x.txt")), None);
        assert_eq!(MediaKind::of(Path::new("sans-extension")), None);
    }
}
