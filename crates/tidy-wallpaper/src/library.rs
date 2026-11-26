//! Bibliothèque locale des fonds d'écran (`wallpapers.json`, versionné).
//!
//! **Les fichiers importés sont référencés par leur chemin, jamais copiés, déplacés ni
//! supprimés.** Pourquoi pas une copie : les vidéos pèsent souvent des centaines de
//! mégaoctets, et une copie doublerait la place prise sans rien protéger (l'utilisateur
//! garde ses fichiers où il veut). Un fichier déplacé ou supprimé hors de Tidy reste
//! dans la bibliothèque, marqué « introuvable » ([`Wallpaper::available`]) ; le
//! retirer de la bibliothèque n'efface que l'entrée (et sa miniature, qui est à Tidy).
//!
//! Doublons : un même fichier (même chemin canonique) n'est importé qu'une fois.
//!
//! Un seul écrivain : la fenêtre principale (page Fonds d'écran). L'agent ne fait que
//! lire ce fichier.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tidy_core::store::Versioned;
use uuid::Uuid;

use crate::MediaKind;

/// Profondeur maximale de l'import d'un dossier (le dossier lui-même : 0).
pub const MAX_DEPTH: usize = 4;
/// Nombre maximal de fichiers examinés lors de l'import d'un dossier : un dossier
/// énorme choisi par erreur ne bloque pas la fenêtre.
pub const MAX_SCANNED: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wallpaper {
    pub id: Uuid,
    /// Fichier de l'utilisateur, à sa place.
    pub path: PathBuf,
    pub kind: MediaKind,
    /// Nom affiché (le nom du fichier sans extension, au départ).
    pub name: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub favorite: bool,
}

impl Wallpaper {
    /// Le fichier existe-t-il encore ?
    pub fn available(&self) -> bool {
        self.path.is_file()
    }
}

/// Filtre de la grille.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Favorites,
    Images,
    Videos,
    Category(String),
}

/// Résultat de l'import d'un fichier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Imported {
    Added(Uuid),
    /// Déjà dans la bibliothèque (identifiant existant).
    Duplicate(Uuid),
    /// Format non accepté, ou pas un fichier.
    Unsupported,
}

/// Bilan de l'import d'un dossier ou de plusieurs fichiers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub added: Vec<Uuid>,
    pub duplicates: usize,
    pub unsupported: usize,
}

impl ImportReport {
    fn record(&mut self, outcome: Imported) {
        match outcome {
            Imported::Added(id) => self.added.push(id),
            Imported::Duplicate(_) => self.duplicates += 1,
            Imported::Unsupported => self.unsupported += 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub schema_version: u32,
    pub items: Vec<Wallpaper>,
    /// Catégories créées par l'utilisateur (ou d'après les dossiers importés), dans
    /// l'ordre d'affichage.
    pub categories: Vec<String>,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            schema_version: Self::CURRENT_VERSION,
            items: Vec::new(),
            categories: Vec::new(),
        }
    }
}

impl Versioned for Library {
    const CURRENT_VERSION: u32 = 1;
    const FILE_NAME: &'static str = "wallpapers.json";
}

/// Chemin canonique (liens et `..` résolus), ou le chemin tel quel s'il n'existe pas.
/// Sous Windows, sans le préfixe `\\?\` que `canonicalize` ajoute : l'Explorateur
/// (« Afficher »), le shell (miniatures) et le fond d'écran ne le comprennent pas.
fn canonical(path: &Path) -> PathBuf {
    let Ok(resolved) = std::fs::canonicalize(path) else {
        return path.to_owned();
    };
    if cfg!(windows)
        && let Some(plain) = resolved.to_str().and_then(without_verbatim)
    {
        return PathBuf::from(plain);
    }
    resolved
}

/// `\\?\C:\dossier` → `C:\dossier`, `\\?\UNC\serveur\partage` → `\\serveur\partage`.
/// `None` pour un chemin sans ce préfixe ou d'une autre forme (volume par GUID…).
fn without_verbatim(path: &str) -> Option<String> {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        return Some(format!(r"\\{rest}"));
    }
    let rest = path.strip_prefix(r"\\?\")?;
    let bytes = rest.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        .then(|| rest.to_owned())
}

fn display_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl Library {
    pub fn get(&self, id: Uuid) -> Option<&Wallpaper> {
        self.items.iter().find(|w| w.id == id)
    }

    fn get_mut(&mut self, id: Uuid) -> Option<&mut Wallpaper> {
        self.items.iter_mut().find(|w| w.id == id)
    }

    /// Fond déjà importé pour ce fichier.
    pub fn find_path(&self, path: &Path) -> Option<&Wallpaper> {
        let wanted = canonical(path);
        // Les chemins enregistrés sont déjà canoniques : pas de nouvel appel au disque
        // par fond (l'import d'un gros dossier devenait quadratique). Une ancienne
        // entrée avec le préfixe `\\?\` (Windows) est reconnue sans lui.
        self.items.iter().find(|w| {
            w.path == wanted
                || w.path
                    .to_str()
                    .and_then(without_verbatim)
                    .is_some_and(|p| Path::new(&p) == wanted)
        })
    }

    /// Importe un fichier (référencé par son chemin canonique), dans `category`.
    pub fn import_file(&mut self, path: &Path, category: Option<&str>) -> Imported {
        if !path.is_file() {
            return Imported::Unsupported;
        }
        let Some(kind) = MediaKind::of(path) else {
            return Imported::Unsupported;
        };
        if let Some(existing) = self.find_path(path) {
            return Imported::Duplicate(existing.id);
        }
        let category = category.and_then(|c| self.add_category(c));
        let id = Uuid::new_v4();
        self.items.push(Wallpaper {
            id,
            path: canonical(path),
            kind,
            name: display_name(path),
            category,
            favorite: false,
        });
        Imported::Added(id)
    }

    /// Importe plusieurs fichiers.
    pub fn import_files(&mut self, paths: &[PathBuf], category: Option<&str>) -> ImportReport {
        let mut report = ImportReport::default();
        for path in paths {
            report.record(self.import_file(path, category));
        }
        report
    }

    /// Importe les images et vidéos d'un dossier et de ses sous-dossiers (jusqu'à
    /// [`MAX_DEPTH`]), dans une catégorie du nom du dossier. Fichiers cachés ignorés.
    pub fn import_dir(&mut self, dir: &Path) -> ImportReport {
        let mut files = Vec::new();
        collect(dir, 0, &mut files);
        files.sort();
        let category = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.is_empty());
        let mut report = ImportReport::default();
        for path in files {
            report.record(self.import_file(&path, category.as_deref()));
        }
        // Une catégorie vide (rien d'importé) ne sert à rien.
        if let Some(c) = &category
            && !self.items.iter().any(|w| w.category.as_ref() == Some(c))
        {
            self.categories.retain(|x| x != c);
        }
        report
    }

    /// Retire un fond de la bibliothèque (le fichier reste où il est).
    pub fn remove(&mut self, id: Uuid) -> Option<Wallpaper> {
        let i = self.items.iter().position(|w| w.id == id)?;
        Some(self.items.remove(i))
    }

    pub fn set_favorite(&mut self, id: Uuid, favorite: bool) {
        if let Some(w) = self.get_mut(id) {
            w.favorite = favorite;
        }
    }

    pub fn rename(&mut self, id: Uuid, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        if let Some(w) = self.get_mut(id) {
            w.name = name.to_owned();
        }
    }

    /// Range un fond dans une catégorie (créée au besoin), ou le sort de toute
    /// catégorie.
    pub fn set_category(&mut self, id: Uuid, category: Option<&str>) {
        let category = category.and_then(|c| self.add_category(c));
        if let Some(w) = self.get_mut(id) {
            w.category = category;
        }
    }

    /// Ajoute une catégorie si elle n'existe pas. Rend son nom nettoyé.
    pub fn add_category(&mut self, name: &str) -> Option<String> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        if !self.categories.iter().any(|c| c == name) {
            self.categories.push(name.to_owned());
        }
        Some(name.to_owned())
    }

    /// Supprime une catégorie : ses fonds restent, sans catégorie.
    pub fn remove_category(&mut self, name: &str) {
        self.categories.retain(|c| c != name);
        for w in &mut self.items {
            if w.category.as_deref() == Some(name) {
                w.category = None;
            }
        }
    }

    /// Fonds qui passent le filtre, dans l'ordre d'import.
    pub fn filtered<'a>(&'a self, filter: &'a Filter) -> impl Iterator<Item = &'a Wallpaper> {
        self.items.iter().filter(move |w| match filter {
            Filter::All => true,
            Filter::Favorites => w.favorite,
            Filter::Images => w.kind == MediaKind::Image,
            Filter::Videos => w.kind == MediaKind::Video,
            Filter::Category(c) => w.category.as_ref() == Some(c),
        })
    }
}

/// Fichiers acceptés de `dir`, récursivement.
fn collect(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH || out.len() >= MAX_SCANNED {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if out.len() >= MAX_SCANNED {
            return;
        }
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            collect(&path, depth + 1, out);
        } else if MediaKind::of(&path).is_some() {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tidy_core::Store;

    use super::*;

    #[test]
    fn verbatim_prefix_is_removed() {
        assert_eq!(
            without_verbatim(r"\\?\C:\Images\été.jpg").as_deref(),
            Some(r"C:\Images\été.jpg")
        );
        assert_eq!(
            without_verbatim(r"\\?\UNC\nas\photos\a.png").as_deref(),
            Some(r"\\nas\photos\a.png")
        );
        assert_eq!(without_verbatim(r"C:\Images\a.jpg"), None);
        assert_eq!(without_verbatim(r"\\?\Volume{1234}\a.jpg"), None);
    }

    fn touch(path: &Path) {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(path, b"x").unwrap();
    }

    #[test]
    fn importing_files_skips_duplicates_and_unknown_formats() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("Plage.JPG");
        let video = dir.path().join("vagues.mp4");
        let text = dir.path().join("notes.txt");
        for p in [&image, &video, &text] {
            touch(p);
        }
        let mut lib = Library::default();
        let report = lib.import_files(
            &[
                image.clone(),
                video.clone(),
                text,
                dir.path().join("absent.png"),
            ],
            None,
        );
        assert_eq!(report.added.len(), 2);
        assert_eq!(report.unsupported, 2);
        let first = report.added[0];
        assert_eq!(lib.get(first).unwrap().name, "Plage");
        assert_eq!(lib.get(first).unwrap().kind, MediaKind::Image);

        // Même fichier, autre écriture du chemin : doublon.
        let again = dir.path().join(".").join("Plage.JPG");
        assert_eq!(lib.import_file(&again, None), Imported::Duplicate(first));
        assert_eq!(lib.items.len(), 2);
    }

    #[test]
    fn importing_a_folder_uses_its_name_as_category() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Nature");
        touch(&root.join("a.png"));
        touch(&root.join("sous/b.mov"));
        touch(&root.join(".cache/c.png"));
        touch(&root.join("d.pdf"));
        let mut lib = Library::default();
        let report = lib.import_dir(&root);
        assert_eq!(report.added.len(), 2);
        assert_eq!(lib.categories, vec!["Nature".to_owned()]);
        assert!(
            lib.items
                .iter()
                .all(|w| w.category.as_deref() == Some("Nature"))
        );
        // Deuxième import : que des doublons, aucune catégorie en plus.
        let report = lib.import_dir(&root);
        assert!(report.added.is_empty());
        assert_eq!(report.duplicates, 2);
        assert_eq!(lib.categories.len(), 1);
        // Les fichiers n'ont pas bougé.
        assert!(root.join("a.png").is_file());
        assert!(root.join("sous/b.mov").is_file());
    }

    #[test]
    fn empty_folder_leaves_no_category() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::default();
        lib.import_dir(dir.path());
        assert!(lib.categories.is_empty());
    }

    #[test]
    fn favorites_categories_and_filters() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.png");
        let b = dir.path().join("b.mp4");
        touch(&a);
        touch(&b);
        let mut lib = Library::default();
        let Imported::Added(ia) = lib.import_file(&a, None) else {
            panic!()
        };
        let Imported::Added(ib) = lib.import_file(&b, None) else {
            panic!()
        };
        lib.set_favorite(ib, true);
        lib.set_category(ia, Some(" Ville "));
        assert_eq!(lib.categories, vec!["Ville".to_owned()]);
        let count = |f: Filter| lib.filtered(&f).count();
        assert_eq!(count(Filter::All), 2);
        assert_eq!(count(Filter::Favorites), 1);
        assert_eq!(count(Filter::Images), 1);
        assert_eq!(count(Filter::Videos), 1);
        assert_eq!(count(Filter::Category("Ville".into())), 1);
        lib.remove_category("Ville");
        assert_eq!(lib.get(ia).unwrap().category, None);
        // Retirer de la bibliothèque ne touche pas au fichier.
        lib.remove(ia);
        assert!(lib.get(ia).is_none());
        assert!(a.is_file());
    }

    #[test]
    fn index_is_versioned_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let file = dir.path().join("x.png");
        touch(&file);
        let mut lib = Library::default();
        lib.import_file(&file, Some("Essai"));
        store.save(&lib).unwrap();
        let text = fs::read_to_string(dir.path().join("wallpapers.json")).unwrap();
        assert!(text.contains("\"schema_version\": 1"));
        assert_eq!(store.load::<Library>(), lib);
        // Une version plus récente est mise de côté, pas écrasée sans trace.
        fs::write(
            dir.path().join("wallpapers.json"),
            r#"{"schema_version": 9}"#,
        )
        .unwrap();
        assert_eq!(store.load::<Library>(), Library::default());
        assert!(dir.path().join("wallpapers.json.bad").exists());
    }
}
