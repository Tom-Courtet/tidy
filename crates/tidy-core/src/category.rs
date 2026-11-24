//! Catégories d'éléments du bureau et extensions par défaut.
//!
//! Les listes viennent de l'annexe A du bilan de l'original (faits techniques), adaptées
//! à macOS. Le classement fin (UTI de macOS, alias, liens de lanceurs de jeux) est fait
//! par le moteur de règles ; ce module ne connaît que les extensions.

use serde::{Deserialize, Serialize};

/// Catégorie d'un élément du bureau. L'ordre est celui de l'écran « Rangement ».
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Application,
    Folder,
    Document,
    Image,
    Video,
    Music,
    Archive,
    Executable,
    WebLink,
    Game,
    Other,
}

impl Category {
    pub const ALL: [Category; 11] = [
        Category::Application,
        Category::Folder,
        Category::Document,
        Category::Image,
        Category::Video,
        Category::Music,
        Category::Archive,
        Category::Executable,
        Category::WebLink,
        Category::Game,
        Category::Other,
    ];

    /// Clé de traduction du nom de la catégorie (`category.<clé>`).
    pub fn key(self) -> &'static str {
        match self {
            Category::Application => "application",
            Category::Folder => "folder",
            Category::Document => "document",
            Category::Image => "image",
            Category::Video => "video",
            Category::Music => "music",
            Category::Archive => "archive",
            Category::Executable => "executable",
            Category::WebLink => "web_link",
            Category::Game => "game",
            Category::Other => "other",
        }
    }

    /// Extensions par défaut (en minuscules, sans point). Vide pour les catégories
    /// reconnues autrement (dossiers, jeux, autres).
    pub fn default_extensions(self) -> &'static [&'static str] {
        match self {
            Category::Application => &["app"],
            Category::Folder | Category::Game | Category::Other => &[],
            Category::Document => &[
                "docx", "doc", "pdf", "txt", "rtf", "docm", "wps", "xls", "xlsx", "xlsm", "ppt",
                "pptx", "csv", "htm", "html", "xml", "mht", "mhtml", "xps", "pages", "numbers",
                "key", "md", "odt", "ods", "odp", "epub",
            ],
            Category::Image => &[
                "jpg", "jpeg", "png", "webp", "svg", "bmp", "raw", "gif", "ai", "apng", "arw",
                "avif", "cr", "cr2", "cr3", "cur", "eps", "heic", "heif", "ico", "icns", "ind",
                "indd", "indt", "jfi", "jfif", "jif", "jpe", "k25", "nef", "nrw", "orf", "pjp",
                "pjpeg", "psd", "rw2", "sr2", "svgz", "tif", "tiff", "dng", "jxl",
            ],
            Category::Video => &[
                "webm", "mkv", "flv", "vob", "ogv", "drc", "gifv", "mng", "avi", "mts", "m2ts",
                "ts", "mov", "qt", "wmv", "yuv", "rm", "rmvb", "viv", "asf", "amv", "mp4", "m4v",
                "mpg", "mp2", "mpeg", "mpe", "mpv", "m2v", "svi", "3gp", "3g2", "mxf", "roq",
                "nsv", "f4v", "f4p", "f4a", "f4b",
            ],
            Category::Music => &[
                "mp3", "aac", "flac", "ogg", "wav", "wma", "8svx", "aa", "aax", "act", "aiff",
                "aif", "alac", "amr", "ape", "au", "awb", "cda", "dss", "dvf", "gsm", "iklax",
                "ivs", "m4a", "m4b", "m4p", "mmf", "movpkg", "mpc", "msv", "nmf", "oga", "mogg",
                "opus", "ra", "rf64", "sln", "tta", "voc", "vox", "wv",
            ],
            Category::Archive => &[
                "zip",
                "rar",
                "7z",
                "iso",
                "gz",
                "gzip",
                "xz",
                "001",
                "cab",
                "txz",
                "lzma",
                "tar",
                "cpio",
                "bz2",
                "bzip2",
                "tbz2",
                "tbz",
                "tgz",
                "tpz",
                "z",
                "taz",
                "lzh",
                "lha",
                "rpm",
                "deb",
                "arj",
                "vhd",
                "vhdx",
                "wim",
                "swm",
                "esd",
                "xar",
                "squashfs",
                "sparsebundle",
                "sparseimage",
            ],
            // Sur macOS : installeurs et scripts exécutables.
            Category::Executable => &[
                "pkg", "mpkg", "dmg", "command", "sh", "tool", "exe", "msi", "bat",
            ],
            Category::WebLink => &["webloc", "url", "inetloc"],
        }
    }
}

/// Extension en minuscules d'un nom de fichier (`None` sans extension ou pour un
/// fichier caché sans extension comme `.zshrc`).
pub fn extension_of(name: &str) -> Option<String> {
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty() || ext.is_empty() {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

/// Catégorie d'après le seul nom et la nature (dossier ou non), avec les listes par
/// défaut. Un paquet `.app` est une application même si c'est un dossier.
pub fn classify_by_extension(name: &str, is_dir: bool) -> Category {
    let ext = extension_of(name);
    if let Some(ext) = &ext {
        for category in Category::ALL {
            if category.default_extensions().contains(&ext.as_str()) {
                return category;
            }
        }
    }
    if is_dir {
        Category::Folder
    } else {
        Category::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_are_lowercase_and_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for category in Category::ALL {
            for ext in category.default_extensions() {
                assert_eq!(*ext, ext.to_ascii_lowercase(), "{ext}");
                assert!(seen.insert(*ext), "extension {ext} dans deux catégories");
            }
        }
    }

    #[test]
    fn classify_common_files() {
        assert_eq!(
            classify_by_extension("Rapport.PDF", false),
            Category::Document
        );
        assert_eq!(classify_by_extension("photo.heic", false), Category::Image);
        assert_eq!(
            classify_by_extension("Safari.app", true),
            Category::Application
        );
        assert_eq!(classify_by_extension("Projets", true), Category::Folder);
        assert_eq!(classify_by_extension("notes", false), Category::Other);
        assert_eq!(classify_by_extension(".zshrc", false), Category::Other);
        assert_eq!(
            classify_by_extension("site.webloc", false),
            Category::WebLink
        );
        assert_eq!(
            classify_by_extension("Install.dmg", false),
            Category::Executable
        );
    }
}
