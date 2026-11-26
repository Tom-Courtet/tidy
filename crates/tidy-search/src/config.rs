//! Ce qu'on indexe : dossiers racines (par défaut et ajoutés) et exclusions.

use std::fs::Metadata;
use std::path::{Path, PathBuf};

use tidy_core::settings::search::SearchSettings;

/// Clés des dossiers indexés par défaut, dans l'ordre de la page Recherche.
pub const DEFAULT_KEYS: [&str; 7] = [
    "desktop",
    "documents",
    "downloads",
    "pictures",
    "music",
    "videos",
    "programs",
];

/// Exclusions de Tidy, toujours appliquées (noms, sans tenir compte de la casse).
pub const BUILTIN_EXCLUSIONS: [&str; 17] = [
    "node_modules",
    ".git",
    ".svn",
    ".hg",
    "__pycache__",
    ".cache",
    "$Recycle.Bin",
    "System Volume Information",
    ".Trash",
    ".Trashes",
    ".Spotlight-V100",
    ".fseventsd",
    ".DocumentRevisions-V100",
    ".DS_Store",
    "Thumbs.db",
    "desktop.ini",
    "~$*",
];

/// Dossiers « paquets » (macOS surtout) : indexés comme un seul élément, sans entrer
/// dedans (une photothèque contient des centaines de milliers de fichiers).
const PACKAGE_EXTENSIONS: [&str; 22] = [
    "app",
    "bundle",
    "framework",
    "plugin",
    "kext",
    "photoslibrary",
    "musiclibrary",
    "tvlibrary",
    "imovielibrary",
    "photolibrary",
    "fcpbundle",
    "logicx",
    "band",
    "xcodeproj",
    "xcworkspace",
    "playground",
    "pkg",
    "mpkg",
    "rtfd",
    "sparsebundle",
    "lrlibrary",
    "lrdata",
];

/// Un dossier indexé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    pub path: PathBuf,
    /// Dossier du bureau (onglet « Bureau »).
    pub desktop: bool,
    /// Dossier de programmes (menu Démarrer, `/Applications`) : ses raccourcis et
    /// liens sont des applications.
    pub programs: bool,
}

/// Dossiers par défaut d'une clé de [`DEFAULT_KEYS`] (aucun, un ou plusieurs ; seuls
/// ceux qui existent).
pub fn default_folders(key: &str) -> Vec<PathBuf> {
    let user = directories::UserDirs::new();
    let user_dir = |pick: fn(&directories::UserDirs) -> Option<&Path>| {
        user.as_ref()
            .and_then(pick)
            .map(Path::to_path_buf)
            .into_iter()
            .collect::<Vec<_>>()
    };
    let dirs = match key {
        "desktop" => tidy_core::store::desktop_dirs(),
        "documents" => user_dir(|u| u.document_dir()),
        "downloads" => user_dir(|u| u.download_dir()),
        "pictures" => user_dir(|u| u.picture_dir()),
        "music" => user_dir(|u| u.audio_dir()),
        "videos" => user_dir(|u| u.video_dir()),
        "programs" => program_dirs(),
        _ => Vec::new(),
    };
    dirs.into_iter().filter(|d| d.is_dir()).collect()
}

/// Dossiers des programmes : menu Démarrer de l'utilisateur et commun (Windows),
/// applications (macOS).
fn program_dirs() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let start = |var: &str| {
            std::env::var_os(var).map(|v| {
                PathBuf::from(v)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Start Menu")
                    .join("Programs")
            })
        };
        start("APPDATA")
            .into_iter()
            .chain(start("ProgramData"))
            .collect()
    }
    #[cfg(target_os = "macos")]
    {
        let mut dirs = vec![
            PathBuf::from("/Applications"),
            PathBuf::from("/System/Applications"),
        ];
        if let Some(home) = directories::BaseDirs::new() {
            dirs.push(home.home_dir().join("Applications"));
        }
        dirs
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Vec::new()
    }
}

/// Exclusions : celles de Tidy et celles de l'utilisateur.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Exclusions {
    /// Motifs de nom en minuscules (`*` et `?` permis).
    names: Vec<String>,
    /// Chemins exclus (et tout ce qui est dessous), en minuscules.
    paths: Vec<PathBuf>,
    /// Indexer les éléments cachés.
    pub index_hidden: bool,
}

impl Exclusions {
    /// `user` : noms (`build`, `*.tmp`) ou chemins complets.
    pub fn new(user: &[String], index_hidden: bool) -> Self {
        let mut names: Vec<String> = BUILTIN_EXCLUSIONS
            .iter()
            .map(|n| n.to_lowercase())
            .collect();
        let mut paths = Vec::new();
        for pattern in user {
            let pattern = pattern.trim();
            if pattern.is_empty() {
                continue;
            }
            if pattern.contains(['/', '\\']) {
                paths.push(PathBuf::from(pattern.to_lowercase()));
            } else {
                names.push(pattern.to_lowercase());
            }
        }
        Self {
            names,
            paths,
            index_hidden,
        }
    }

    /// Le nom seul suffit à exclure l'élément (motif, nom caché en `.`).
    pub fn excludes_name(&self, name: &str) -> bool {
        if !self.index_hidden && name.starts_with('.') {
            return true;
        }
        let lower = name.to_lowercase();
        self.names.iter().any(|p| wildcard(p, &lower))
    }

    /// Le chemin est sous un chemin exclu.
    pub fn excludes_path(&self, path: &Path) -> bool {
        if self.paths.is_empty() {
            return false;
        }
        let lower = PathBuf::from(path.to_string_lossy().to_lowercase());
        self.paths.iter().any(|p| lower.starts_with(p))
    }

    /// Élément caché pour le système (attribut « caché » ou « système » de Windows,
    /// drapeau `hidden` de macOS).
    pub fn excludes_metadata(&self, meta: &Metadata) -> bool {
        !self.index_hidden && is_hidden(meta)
    }
}

#[cfg(windows)]
fn is_hidden(meta: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    meta.file_attributes() & (HIDDEN | SYSTEM) != 0
}

#[cfg(target_os = "macos")]
fn is_hidden(meta: &Metadata) -> bool {
    use std::os::macos::fs::MetadataExt;
    const UF_HIDDEN: u32 = 0x8000;
    meta.st_flags() & UF_HIDDEN != 0
}

#[cfg(not(any(windows, target_os = "macos")))]
fn is_hidden(_meta: &Metadata) -> bool {
    false
}

/// Dossier « paquet », indexé sans y entrer.
pub fn is_package(name: &str) -> bool {
    tidy_core::category::extension_of(name)
        .is_some_and(|ext| PACKAGE_EXTENSIONS.contains(&ext.as_str()))
}

/// Motif simple : `*` (n'importe quelle suite) et `?` (un caractère).
fn wildcard(pattern: &str, text: &str) -> bool {
    if !pattern.contains(['*', '?']) {
        return pattern == text;
    }
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            pi += 1;
            mark = ti;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// Ce qu'on indexe.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    pub roots: Vec<Root>,
    pub exclusions: Exclusions,
}

impl Config {
    /// D'après les réglages : dossiers par défaut gardés, puis dossiers ajoutés.
    pub fn from_settings(settings: &SearchSettings) -> Self {
        let mut roots: Vec<Root> = Vec::new();
        let mut add = |path: PathBuf, desktop: bool, programs: bool| {
            if let Some(r) = roots.iter_mut().find(|r| r.path == path) {
                r.desktop |= desktop;
                r.programs |= programs;
            } else {
                roots.push(Root {
                    path,
                    desktop,
                    programs,
                });
            }
        };
        for key in DEFAULT_KEYS {
            if settings.disabled_defaults.iter().any(|k| k == key) {
                continue;
            }
            for path in default_folders(key) {
                add(path, key == "desktop", key == "programs");
            }
        }
        for path in &settings.folders {
            if path.is_dir() {
                add(path.clone(), false, false);
            }
        }
        Self {
            roots,
            exclusions: Exclusions::new(&settings.exclusions, settings.index_hidden),
        }
    }

    /// Empreinte stable (FNV-1a) : un cache écrit avec une autre configuration est
    /// ignoré.
    pub fn signature(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |bytes: &[u8]| {
            for b in bytes {
                h ^= u64::from(*b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
            h ^= 0xff;
            h = h.wrapping_mul(0x0100_0000_01b3);
        };
        for r in &self.roots {
            eat(r.path.to_string_lossy().as_bytes());
            eat(&[u8::from(r.desktop), u8::from(r.programs)]);
        }
        for n in &self.exclusions.names {
            eat(n.as_bytes());
        }
        for p in &self.exclusions.paths {
            eat(p.to_string_lossy().as_bytes());
        }
        eat(&[u8::from(self.exclusions.index_hidden)]);
        h
    }

    /// Racine qui contient `path`, s'il y en a une.
    pub fn root_of(&self, path: &Path) -> Option<&Root> {
        self.roots.iter().find(|r| path.starts_with(&r.path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        assert!(wildcard("*.tmp", "a.tmp"));
        assert!(wildcard("~$*", "~$rapport.docx"));
        assert!(wildcard("a?c", "abc"));
        assert!(!wildcard("*.tmp", "a.tmpx"));
        assert!(wildcard("build", "build"));
        assert!(!wildcard("build", "builds"));
    }

    #[test]
    fn exclusions() {
        let ex = Exclusions::new(&["Build".into(), "*.TMP".into(), "/x/Secret".into()], false);
        assert!(ex.excludes_name("node_modules"));
        assert!(ex.excludes_name("NODE_MODULES"));
        assert!(ex.excludes_name(".zshrc"), "caché");
        assert!(ex.excludes_name("build"));
        assert!(ex.excludes_name("note.tmp"));
        assert!(!ex.excludes_name("rapport.pdf"));
        assert!(ex.excludes_path(Path::new("/x/secret/a.txt")));
        assert!(!ex.excludes_path(Path::new("/x/secrets/a.txt")));
        let with_hidden = Exclusions::new(&[], true);
        assert!(!with_hidden.excludes_name(".zshrc"));
        assert!(with_hidden.excludes_name(".git"), "toujours exclu");
    }

    #[test]
    fn packages() {
        assert!(is_package("Safari.app"));
        assert!(is_package("Photos Library.photoslibrary"));
        assert!(!is_package("Projets"));
    }

    #[test]
    fn signature_follows_the_config() {
        let a = Config {
            roots: vec![Root {
                path: "/a".into(),
                desktop: true,
                programs: false,
            }],
            exclusions: Exclusions::new(&[], false),
        };
        let mut b = a.clone();
        assert_eq!(a.signature(), b.signature());
        b.exclusions = Exclusions::new(&["x".into()], false);
        assert_ne!(a.signature(), b.signature());
    }

    #[test]
    fn added_folders_are_roots() {
        let dir = tempfile::tempdir().unwrap();
        let settings = SearchSettings {
            disabled_defaults: DEFAULT_KEYS.iter().map(|k| k.to_string()).collect(),
            folders: vec![dir.path().to_path_buf(), dir.path().join("absent")],
            ..SearchSettings::default()
        };
        let config = Config::from_settings(&settings);
        assert_eq!(config.roots.len(), 1, "seuls les dossiers existants");
        assert_eq!(config.roots[0].path, dir.path());
    }
}
