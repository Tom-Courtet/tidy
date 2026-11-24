//! Lecture du dossier du bureau : ce que le moteur et l'affichage savent de chaque
//! élément.
//!
//! Le classement part de l'extension ([`crate::category`]), puis regarde le contenu de
//! quelques petits fichiers :
//! - raccourci Windows `.lnk` : cible `.exe`, `.bat`, `.cmd` ou `.com` = application ;
//!   autre cible = application aussi, mais la catégorie de la cible est notée
//!   ([`DesktopEntry::alias_of`]) pour l'option « ranger aussi les raccourcis » ; cible
//!   illisible = application ;
//! - lien Internet `.url` (fichier INI, champ `URL=`) et `.webloc` / `.inetloc` (liste de
//!   propriétés, clé `URL`) : jeu si l'adresse est celle d'un lanceur de jeux, sinon lien
//!   web.
//!
//! Seuls les premiers Ko de ces fichiers sont lus ; rien n'est jamais écrit.

use std::io::Read;
use std::path::Path;

use jiff::Timestamp;

use crate::category::{
    Category, GAME_URL_SCHEMES, PROGRAM_EXTENSIONS, classify_by_extension, classify_url,
    extension_of,
};
use crate::rules::ItemInfo;

/// Taille maximale lue dans un raccourci ou un lien.
const MAX_LINK_BYTES: u64 = 64 * 1024;

/// Élément du bureau, avec ce qui ne tient pas dans [`ItemInfo`].
#[derive(Debug, Clone, PartialEq)]
pub struct DesktopEntry {
    pub info: ItemInfo,
    /// Pour un raccourci (`.lnk`) qui ne vise pas un programme : catégorie de sa cible
    /// (document, dossier…). Sert à l'option « ranger aussi les raccourcis » d'une
    /// catégorie ([`crate::settings::CategorySetting::include_aliases`]).
    pub alias_of: Option<Category>,
}

impl DesktopEntry {
    /// Élément sans contenu particulier (tests, éléments connus par leur seul nom).
    pub fn new(info: ItemInfo) -> Self {
        Self {
            info,
            alias_of: None,
        }
    }
}

/// Éléments visibles du dossier `dir` (fichiers cachés et `desktop.ini` exclus), triés
/// par nom.
pub fn scan_dir(dir: &Path) -> std::io::Result<Vec<ItemInfo>> {
    Ok(scan_entries(dir)?.into_iter().map(|e| e.info).collect())
}

/// Comme [`scan_dir`], avec la catégorie de la cible des raccourcis.
pub fn scan_entries(dir: &Path) -> std::io::Result<Vec<DesktopEntry>> {
    let mut items = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if let Some(item) = read_entry(&entry.path()) {
            items.push(item);
        }
    }
    items.sort_by_key(|e| e.info.name.to_lowercase());
    Ok(items)
}

/// Lit un élément du bureau. `None` : élément caché, système ou disparu.
pub fn read_entry(path: &Path) -> Option<DesktopEntry> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    if is_hidden_name(&name) {
        return None;
    }
    // Lien symbolique cassé : on le montre quand même, comme le système.
    let meta = std::fs::metadata(path)
        .or_else(|_| std::fs::symlink_metadata(path))
        .ok()?;
    if is_hidden_by_attributes(&meta) {
        return None;
    }
    let is_dir = meta.is_dir();
    let (category, alias_of) = classify_path(path, &name, is_dir);
    Some(DesktopEntry {
        info: ItemInfo {
            category,
            size: if is_dir { 0 } else { meta.len() },
            modified: meta
                .modified()
                .ok()
                .and_then(|t| Timestamp::try_from(t).ok()),
            uti: None,
            is_dir,
            name,
        },
        alias_of,
    })
}

/// Fichiers système qu'un bureau n'affiche pas.
pub fn is_hidden_name(name: &str) -> bool {
    name.starts_with('.')
        || name.eq_ignore_ascii_case("desktop.ini")
        || name.eq_ignore_ascii_case("thumbs.db")
        || name == "Icon\r"
}

/// Fichier de passage (téléchargement en cours, verrou d'Office) : ni rangé ni signalé
/// comme non affecté ; il sera traité sous son nom définitif.
pub fn is_transient_name(name: &str) -> bool {
    const TEMP_EXTENSIONS: [&str; 7] = [
        "crdownload",
        "part",
        "partial",
        "download",
        "opdownload",
        "tmp",
        "!ut",
    ];
    name.starts_with("~$")
        || extension_of(name).is_some_and(|ext| TEMP_EXTENSIONS.contains(&ext.as_str()))
}

/// Attributs « caché » ou « système » de Windows (l'Explorateur ne les montre pas).
#[cfg(windows)]
fn is_hidden_by_attributes(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    meta.file_attributes() & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0
}

#[cfg(not(windows))]
fn is_hidden_by_attributes(_meta: &std::fs::Metadata) -> bool {
    false
}

/// Catégorie d'un élément, en lisant le contenu des raccourcis et des liens.
fn classify_path(path: &Path, name: &str, is_dir: bool) -> (Category, Option<Category>) {
    let base = classify_by_extension(name, is_dir);
    if is_dir {
        return (base, None);
    }
    match extension_of(name).as_deref() {
        Some("lnk") => read_head(path)
            .and_then(|bytes| lnk::target(&bytes))
            .map_or((Category::Application, None), |t| shortcut_category(&t)),
        Some("url") => {
            let category = read_head(path)
                .and_then(|bytes| url_of_internet_shortcut(&String::from_utf8_lossy(&bytes)))
                .map_or(Category::WebLink, |url| classify_url(&url));
            (category, None)
        }
        Some("webloc" | "inetloc") => {
            let category = read_head(path).map_or(Category::WebLink, |b| plist_link_category(&b));
            (category, None)
        }
        _ => (base, None),
    }
}

/// Catégorie d'un raccourci d'après sa cible.
fn shortcut_category(target: &lnk::Target) -> (Category, Option<Category>) {
    let file_name = target.path.rsplit(['\\', '/']).next().unwrap_or_default();
    match extension_of(file_name) {
        Some(ext) if PROGRAM_EXTENSIONS.contains(&ext.as_str()) => (Category::Application, None),
        // Cible vide (élément virtuel du Shell : Ce PC, Corbeille…) : application.
        _ if file_name.is_empty() => (Category::Application, None),
        _ => (
            Category::Application,
            Some(classify_by_extension(file_name, target.is_dir)),
        ),
    }
}

/// Premiers Ko d'un fichier.
fn read_head(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_LINK_BYTES).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Adresse d'un raccourci Internet de Windows (`[InternetShortcut]`, `URL=…`).
pub fn url_of_internet_shortcut(text: &str) -> Option<String> {
    let mut section = String::new();
    let mut fallback = None;
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim().to_ascii_lowercase();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("url") {
            continue;
        }
        let value = value.trim().to_owned();
        if section == "internetshortcut" {
            return Some(value);
        }
        fallback.get_or_insert(value);
    }
    fallback
}

/// Catégorie d'un `.webloc` / `.inetloc` (liste de propriétés XML ou binaire).
pub fn plist_link_category(bytes: &[u8]) -> Category {
    if bytes.starts_with(b"bplist") {
        // Format binaire : on cherche simplement un préfixe de lanceur de jeux.
        let lower: Vec<u8> = bytes.iter().map(u8::to_ascii_lowercase).collect();
        let is_game = GAME_URL_SCHEMES
            .iter()
            .any(|scheme| lower.windows(scheme.len()).any(|w| w == scheme.as_bytes()));
        return if is_game {
            Category::Game
        } else {
            Category::WebLink
        };
    }
    url_of_xml_plist(&String::from_utf8_lossy(bytes))
        .map_or(Category::WebLink, |url| classify_url(&url))
}

/// Valeur de la clé `URL` d'une liste de propriétés XML.
pub fn url_of_xml_plist(text: &str) -> Option<String> {
    let after_key = &text[text.find("<key>URL</key>")? + "<key>URL</key>".len()..];
    let start = after_key.find("<string>")? + "<string>".len();
    let rest = &after_key[start..];
    let value = &rest[..rest.find("</string>")?];
    Some(
        value
            .trim()
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">"),
    )
}

/// Lecture minimale des raccourcis Windows (format « Shell Link », MS-SHLLINK) : seule
/// la cible nous intéresse.
pub mod lnk {
    /// Cible d'un raccourci.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Target {
        /// Chemin tel qu'écrit dans le raccourci (peut contenir `%ProgramFiles%` ou
        /// être relatif).
        pub path: String,
        /// La cible est un dossier (attributs enregistrés dans le raccourci).
        pub is_dir: bool,
    }

    const HEADER_SIZE: usize = 0x4C;
    const LINK_CLSID: [u8; 16] = [
        0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x46,
    ];
    const HAS_TARGET_ID_LIST: u32 = 0x1;
    const HAS_LINK_INFO: u32 = 0x2;
    const HAS_NAME: u32 = 0x4;
    const HAS_RELATIVE_PATH: u32 = 0x8;
    const HAS_WORKING_DIR: u32 = 0x10;
    const HAS_ARGUMENTS: u32 = 0x20;
    const HAS_ICON_LOCATION: u32 = 0x40;
    const IS_UNICODE: u32 = 0x80;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    const VOLUME_ID_AND_LOCAL_BASE_PATH: u32 = 0x1;
    const ENVIRONMENT_BLOCK: u32 = 0xA000_0001;

    fn u16_at(b: &[u8], off: usize) -> Option<u16> {
        Some(u16::from_le_bytes(
            b.get(off..off.checked_add(2)?)?.try_into().ok()?,
        ))
    }

    fn u32_at(b: &[u8], off: usize) -> Option<u32> {
        Some(u32::from_le_bytes(
            b.get(off..off.checked_add(4)?)?.try_into().ok()?,
        ))
    }

    /// Chaîne d'octets terminée par un zéro, à partir de `off`.
    fn ansi_z(b: &[u8], off: usize) -> Option<String> {
        let bytes = b.get(off..)?;
        let end = bytes.iter().position(|&c| c == 0).unwrap_or(bytes.len());
        Some(String::from_utf8_lossy(&bytes[..end]).into_owned())
    }

    /// Chaîne UTF-16 terminée par un zéro, à partir de `off` (au plus `max` unités).
    fn utf16_z(b: &[u8], off: usize, max: usize) -> Option<String> {
        let mut units = Vec::new();
        let mut i = off;
        while units.len() < max {
            let unit = u16_at(b, i)?;
            if unit == 0 {
                break;
            }
            units.push(unit);
            i += 2;
        }
        Some(String::from_utf16_lossy(&units))
    }

    /// Cible du raccourci `bytes`, ou `None` si ce n'est pas un raccourci lisible ou s'il
    /// ne donne pas de chemin (raccourcis « annoncés » des installeurs MSI, éléments
    /// virtuels du Shell).
    pub fn target(b: &[u8]) -> Option<Target> {
        if u32_at(b, 0)? as usize != HEADER_SIZE || b.get(4..20)? != LINK_CLSID {
            return None;
        }
        let flags = u32_at(b, 0x14)?;
        let is_dir = u32_at(b, 0x18)? & FILE_ATTRIBUTE_DIRECTORY != 0;
        let mut off = HEADER_SIZE;
        if flags & HAS_TARGET_ID_LIST != 0 {
            off += 2 + usize::from(u16_at(b, off)?);
        }
        let mut local_path = None;
        if flags & HAS_LINK_INFO != 0 {
            let start = off;
            let size = u32_at(b, start)? as usize;
            let header = u32_at(b, start + 4)? as usize;
            let info_flags = u32_at(b, start + 8)?;
            if info_flags & VOLUME_ID_AND_LOCAL_BASE_PATH != 0 {
                let base_off = u32_at(b, start + 16)? as usize;
                let suffix_off = u32_at(b, start + 24)? as usize;
                let unicode = if header >= 0x24 {
                    let base_u = u32_at(b, start + 28)? as usize;
                    let suffix_u = u32_at(b, start + 32)? as usize;
                    (base_u != 0).then(|| {
                        let base = utf16_z(b, start + base_u, 1024).unwrap_or_default();
                        let suffix = if suffix_u != 0 {
                            utf16_z(b, start + suffix_u, 1024).unwrap_or_default()
                        } else {
                            String::new()
                        };
                        base + &suffix
                    })
                } else {
                    None
                };
                local_path = unicode.or_else(|| {
                    let base = ansi_z(b, start + base_off)?;
                    let suffix = ansi_z(b, start + suffix_off).unwrap_or_default();
                    Some(base + &suffix)
                });
            }
            off = start.checked_add(size)?;
        }
        let unicode = flags & IS_UNICODE != 0;
        let mut relative = None;
        for bit in [
            HAS_NAME,
            HAS_RELATIVE_PATH,
            HAS_WORKING_DIR,
            HAS_ARGUMENTS,
            HAS_ICON_LOCATION,
        ] {
            if flags & bit == 0 {
                continue;
            }
            let count = usize::from(u16_at(b, off)?);
            off += 2;
            let len = if unicode { count * 2 } else { count };
            let raw = b.get(off..off + len)?;
            if bit == HAS_RELATIVE_PATH {
                relative = Some(if unicode {
                    let units: Vec<u16> = raw
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| u16::from_le_bytes(*c))
                        .collect();
                    String::from_utf16_lossy(&units)
                } else {
                    String::from_utf8_lossy(raw).into_owned()
                });
            }
            off += len;
        }
        // Blocs supplémentaires : le chemin avec variables d'environnement.
        let mut env_path = None;
        while let (Some(size), Some(signature)) = (u32_at(b, off), u32_at(b, off + 4)) {
            let size = size as usize;
            if size < 8 {
                break;
            }
            if signature == ENVIRONMENT_BLOCK {
                env_path = utf16_z(b, off + 8 + 260, 260)
                    .filter(|s| !s.is_empty())
                    .or_else(|| ansi_z(b.get(..off + 8 + 260)?, off + 8));
            }
            off += size;
        }
        [local_path, env_path, relative]
            .into_iter()
            .flatten()
            .find(|p| !p.is_empty())
            .map(|path| Target { path, is_dir })
    }

    /// Fabrique de raccourcis pour les tests.
    #[cfg(test)]
    pub(crate) mod build {
        use super::*;

        /// En-tête avec ces drapeaux et attributs.
        pub fn header(flags: u32, attributes: u32) -> Vec<u8> {
            let mut b = vec![0u8; HEADER_SIZE];
            b[0..4].copy_from_slice(&(HEADER_SIZE as u32).to_le_bytes());
            b[4..20].copy_from_slice(&LINK_CLSID);
            b[0x14..0x18].copy_from_slice(&flags.to_le_bytes());
            b[0x18..0x1C].copy_from_slice(&attributes.to_le_bytes());
            b
        }

        /// Raccourci vers `path` (chemin local en ANSI), avec une liste d'identifiants
        /// factice devant, comme ceux de l'Explorateur.
        pub fn local(path: &str, is_dir: bool) -> Vec<u8> {
            let attributes = if is_dir {
                FILE_ATTRIBUTE_DIRECTORY
            } else {
                0x20
            };
            let mut b = header(HAS_TARGET_ID_LIST | HAS_LINK_INFO | IS_UNICODE, attributes);
            b.extend_from_slice(&4u16.to_le_bytes());
            b.extend_from_slice(&[0xAA; 4]);
            let header_size = 0x1Cu32;
            let base_off = header_size;
            let suffix_off = base_off + path.len() as u32 + 1;
            let size = suffix_off + 1;
            for v in [size, header_size, 1, 0, base_off, 0, suffix_off] {
                b.extend_from_slice(&v.to_le_bytes());
            }
            b.extend_from_slice(path.as_bytes());
            b.extend_from_slice(&[0, 0]);
            // Bloc de fin.
            b.extend_from_slice(&0u32.to_le_bytes());
            b
        }

        /// Raccourci qui ne donne qu'un chemin relatif (UTF-16) et un nom.
        pub fn relative(path: &str) -> Vec<u8> {
            let mut b = header(HAS_NAME | HAS_RELATIVE_PATH | IS_UNICODE, 0x20);
            for s in ["Description", path] {
                let units: Vec<u16> = s.encode_utf16().collect();
                b.extend_from_slice(&(units.len() as u16).to_le_bytes());
                for u in units {
                    b.extend_from_slice(&u.to_le_bytes());
                }
            }
            b
        }

        /// Raccourci avec seulement un bloc de variables d'environnement.
        pub fn environment(path: &str) -> Vec<u8> {
            let mut b = header(0, 0x20);
            b.extend_from_slice(&0x314u32.to_le_bytes());
            b.extend_from_slice(&ENVIRONMENT_BLOCK.to_le_bytes());
            let mut ansi = vec![0u8; 260];
            ansi[..path.len()].copy_from_slice(path.as_bytes());
            b.extend_from_slice(&ansi);
            let mut wide = vec![0u8; 520];
            for (i, u) in path.encode_utf16().enumerate() {
                wide[i * 2..i * 2 + 2].copy_from_slice(&u.to_le_bytes());
            }
            b.extend_from_slice(&wide);
            b.extend_from_slice(&0u32.to_le_bytes());
            b
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn reads_the_local_path() {
            let t = target(&build::local(r"C:\Program Files\App\app.exe", false)).unwrap();
            assert_eq!(t.path, r"C:\Program Files\App\app.exe");
            assert!(!t.is_dir);
            let t = target(&build::local(r"D:\Projets", true)).unwrap();
            assert!(t.is_dir);
        }

        #[test]
        fn falls_back_to_relative_and_environment_paths() {
            let t = target(&build::relative(r"..\..\Jeux\jeu.bat")).unwrap();
            assert_eq!(t.path, r"..\..\Jeux\jeu.bat");
            let t = target(&build::environment(r"%ProgramFiles%\Outil\outil.cmd")).unwrap();
            assert_eq!(t.path, r"%ProgramFiles%\Outil\outil.cmd");
        }

        #[test]
        fn refuses_garbage_and_truncated_files() {
            assert_eq!(target(b""), None);
            assert_eq!(target(b"pas un raccourci du tout, juste du texte"), None);
            let full = build::local(r"C:\a.exe", false);
            for len in 0..full.len() - 4 {
                // Jamais de panique, quelle que soit la coupure.
                let _ = target(&full[..len]);
            }
            // Raccourci sans chemin (annoncé par un installeur) : pas de cible.
            assert_eq!(target(&build::header(0, 0)), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_a_fake_desktop() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Rapport.pdf"), b"x").unwrap();
        std::fs::write(dir.path().join(".DS_Store"), b"x").unwrap();
        std::fs::write(dir.path().join("desktop.ini"), b"x").unwrap();
        std::fs::create_dir(dir.path().join("Projets")).unwrap();
        let items = scan_dir(dir.path()).unwrap();
        let names: Vec<_> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["Projets", "Rapport.pdf"]);
        assert_eq!(items[0].category, Category::Folder);
        assert_eq!(items[1].category, Category::Document);
        assert!(items[1].modified.is_some());
    }

    #[test]
    fn classifies_shortcuts_and_links_by_content() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, bytes: &[u8]| std::fs::write(dir.path().join(name), bytes);
        write(
            "Firefox.lnk",
            &lnk::build::local(r"C:\Program Files\Mozilla Firefox\firefox.exe", false),
        )
        .unwrap();
        write("Script.lnk", &lnk::build::relative(r"..\outils\go.BAT")).unwrap();
        write(
            "Rapport.lnk",
            &lnk::build::local(r"C:\Users\moi\Documents\Rapport.docx", false),
        )
        .unwrap();
        write("Projets.lnk", &lnk::build::local(r"D:\Projets", true)).unwrap();
        write("Cassé.lnk", b"n'importe quoi").unwrap();
        write(
            "Dota 2.url",
            b"[{000214A0-0000-0000-C000-000000000046}]\r\nProp3=19,0\r\n[InternetShortcut]\r\nIDList=\r\nURL=steam://rungameid/570\r\nIconIndex=0\r\n",
        )
        .unwrap();
        write(
            "Fortnite.url",
            b"[InternetShortcut]\nURL=com.epicgames.launcher://apps/Fortnite?action=launch\n",
        )
        .unwrap();
        write(
            "Wikipédia.url",
            b"[InternetShortcut]\nURL=https://fr.wikipedia.org/\n",
        )
        .unwrap();
        write(
            "Roblox.webloc",
            br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>URL</key><string>roblox://placeId=1</string></dict></plist>"#,
        )
        .unwrap();
        write(
            "Rust.webloc",
            br#"<plist version="1.0"><dict><key>URL</key><string>https://www.rust-lang.org/?a=1&amp;b=2</string></dict></plist>"#,
        )
        .unwrap();
        write(
            "Binaire.webloc",
            b"bplist00\xd1\x01\x02SURL_\x10\x0fbattlenet://WoW",
        )
        .unwrap();

        let entries = scan_entries(dir.path()).unwrap();
        let find = |name: &str| {
            let e = entries.iter().find(|e| e.info.name == name).unwrap();
            (e.info.category, e.alias_of)
        };
        assert_eq!(find("Firefox.lnk"), (Category::Application, None));
        assert_eq!(find("Script.lnk"), (Category::Application, None));
        assert_eq!(
            find("Rapport.lnk"),
            (Category::Application, Some(Category::Document))
        );
        assert_eq!(
            find("Projets.lnk"),
            (Category::Application, Some(Category::Folder))
        );
        assert_eq!(find("Cassé.lnk"), (Category::Application, None));
        assert_eq!(find("Dota 2.url"), (Category::Game, None));
        assert_eq!(find("Fortnite.url"), (Category::Game, None));
        assert_eq!(find("Wikipédia.url"), (Category::WebLink, None));
        assert_eq!(find("Roblox.webloc"), (Category::Game, None));
        assert_eq!(find("Rust.webloc"), (Category::WebLink, None));
        assert_eq!(find("Binaire.webloc"), (Category::Game, None));
    }

    #[test]
    fn internet_shortcut_parsing() {
        assert_eq!(
            url_of_internet_shortcut("[Autre]\nURL=faux\n[InternetShortcut]\nURL = vrai \n"),
            Some("vrai".into())
        );
        assert_eq!(url_of_internet_shortcut("URL=seul"), Some("seul".into()));
        assert_eq!(url_of_internet_shortcut("[InternetShortcut]\n"), None);
        assert_eq!(
            url_of_xml_plist("<key>URL</key>\n\t<string> https://a.b/?x&amp;y </string>"),
            Some("https://a.b/?x&y".into())
        );
        assert_eq!(url_of_xml_plist("<key>URL</key>"), None);
    }

    #[test]
    fn transient_files() {
        assert!(is_transient_name("film.mp4.crdownload"));
        assert!(is_transient_name("archive.zip.part"));
        assert!(is_transient_name("~$Rapport.docx"));
        assert!(!is_transient_name("Rapport.docx"));
        assert!(!is_transient_name("partie"));
    }
}
