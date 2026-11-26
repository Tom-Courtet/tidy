//! Sources de fonds en ligne, branchables derrière [`WallpaperSource`].
//!
//! **Désactivées par défaut** : aucune requête réseau tant que l'utilisateur n'en a pas
//! activé une (réglage `wallpaper.online`). Exemple fourni : l'image du jour de Bing.
//! Les images téléchargées vont dans `<données>/wallpapers/online/<source>/` (fichiers
//! de Tidy).

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// Une source d'images en ligne.
pub trait WallpaperSource: Send + Sync {
    /// Identifiant stable (`"bing"`), utilisé dans les réglages.
    fn id(&self) -> &'static str;
    /// Télécharge l'image du moment dans `dir` (si elle n'y est pas déjà) et rend son
    /// chemin.
    fn fetch(&self, dir: &Path) -> Result<PathBuf>;
}

/// Sources connues (une ligne par source).
pub fn sources() -> Vec<Box<dyn WallpaperSource>> {
    vec![Box::new(BingDaily::default())]
}

pub fn by_id(id: &str) -> Option<Box<dyn WallpaperSource>> {
    sources().into_iter().find(|s| s.id() == id)
}

/// Dossier des téléchargements d'une source.
pub fn download_dir(data_root: &Path, source: &str) -> PathBuf {
    crate::cache_dir(data_root).join("online").join(source)
}

/// Dernière image téléchargée d'une source (nom de fichier le plus grand : ils
/// commencent par la date).
pub fn latest(data_root: &Path, source: &str) -> Option<PathBuf> {
    std::fs::read_dir(download_dir(data_root, source))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jpg"))
        .max()
}

/// Ne garde que les `keep` images les plus récentes d'une source.
pub fn prune(data_root: &Path, source: &str, keep: usize) {
    let dir = download_dir(data_root, source);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jpg"))
        .collect();
    files.sort();
    let excess = files.len().saturating_sub(keep);
    for old in files.into_iter().take(excess) {
        if old.starts_with(&dir) {
            let _ = std::fs::remove_file(old);
        }
    }
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(60);
/// Taille maximale d'une image téléchargée.
const MAX_BYTES: u64 = 30 * 1024 * 1024;

fn agent() -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
    let tls = TlsConfig::builder()
        .provider(TlsProvider::NativeTls)
        .root_certs(RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(TOTAL_TIMEOUT))
        .user_agent(concat!("Tidy/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Image du jour de Bing.
#[derive(Debug, Clone)]
pub struct BingDaily {
    /// Marché (`fr-FR`, `en-US`…) : la photo et sa légende en dépendent.
    pub market: String,
}

impl Default for BingDaily {
    fn default() -> Self {
        Self {
            market: "fr-FR".into(),
        }
    }
}

/// Image décrite par l'API de Bing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BingImage {
    /// Date de l'image (`AAAAMMJJ`).
    pub date: String,
    /// Adresse complète de l'image en haute définition.
    pub url: String,
    pub title: String,
}

#[derive(Deserialize)]
struct BingReply {
    images: Vec<BingEntry>,
}

#[derive(Deserialize)]
struct BingEntry {
    #[serde(default)]
    startdate: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    urlbase: String,
    #[serde(default)]
    title: String,
}

/// Lit la réponse de `HPImageArchive.aspx?format=js`.
pub fn parse_bing(json: &str) -> Result<BingImage> {
    let reply: BingReply = serde_json::from_str(json).context("réponse de Bing illisible")?;
    let entry = reply.images.into_iter().next().context("aucune image")?;
    // Version UHD si l'adresse de base est donnée, sinon l'adresse telle quelle.
    let path = if entry.urlbase.is_empty() {
        entry.url
    } else {
        format!("{}_UHD.jpg", entry.urlbase)
    };
    if !path.starts_with('/') || entry.startdate.len() != 8 {
        bail!("réponse de Bing inattendue");
    }
    if !entry.startdate.bytes().all(|b| b.is_ascii_digit()) {
        bail!("date de Bing illisible");
    }
    Ok(BingImage {
        date: entry.startdate,
        url: format!("https://www.bing.com{path}"),
        title: entry.title,
    })
}

impl WallpaperSource for BingDaily {
    fn id(&self) -> &'static str {
        "bing"
    }

    fn fetch(&self, dir: &Path) -> Result<PathBuf> {
        let agent = agent();
        let api = format!(
            "https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=1&mkt={}",
            self.market
        );
        let json = agent
            .get(&api)
            .call()
            .context("Bing injoignable")?
            .into_body()
            .read_to_string()
            .context("réponse de Bing")?;
        let image = parse_bing(&json)?;
        let target = dir.join(format!("{}.jpg", image.date));
        if target.is_file() {
            return Ok(target);
        }
        let bytes = agent
            .get(&image.url)
            .call()
            .context("téléchargement de l'image de Bing")?
            .into_body()
            .with_config()
            .limit(MAX_BYTES)
            .read_to_vec()
            .context("image de Bing")?;
        // Une vraie image JPEG, pas une page d'erreur.
        if !bytes.starts_with(&[0xFF, 0xD8]) {
            bail!("Bing n'a pas renvoyé une image JPEG");
        }
        tidy_core::store::write_atomic(&target, &bytes)?;
        log::info!("image du jour de Bing : {} ({})", image.title, image.date);
        Ok(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bing_reply_is_parsed() {
        let json = r#"{"images":[{"startdate":"20261126","url":"/th?id=OHR.Phare_FR-FR123_1920x1080.jpg&rf=x","urlbase":"/th?id=OHR.Phare_FR-FR123","title":"Un phare"}],"tooltips":{}}"#;
        let img = parse_bing(json).unwrap();
        assert_eq!(img.date, "20261126");
        assert_eq!(
            img.url,
            "https://www.bing.com/th?id=OHR.Phare_FR-FR123_UHD.jpg"
        );
        assert_eq!(img.title, "Un phare");
    }

    #[test]
    fn odd_bing_replies_are_refused() {
        assert!(parse_bing("{}").is_err());
        assert!(parse_bing(r#"{"images":[]}"#).is_err());
        // Adresse absolue vers un autre site : refusée.
        let other =
            r#"{"images":[{"startdate":"20261126","url":"https://ailleurs.example/x.jpg"}]}"#;
        assert!(parse_bing(other).is_err());
        let bad_date = r#"{"images":[{"startdate":"../../x","url":"/x.jpg"}]}"#;
        assert!(parse_bing(bad_date).is_err());
    }

    #[test]
    fn latest_and_prune_keep_recent_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let d = download_dir(dir.path(), "bing");
        std::fs::create_dir_all(&d).unwrap();
        for day in ["20261124", "20261125", "20261126"] {
            std::fs::write(d.join(format!("{day}.jpg")), b"x").unwrap();
        }
        assert_eq!(latest(dir.path(), "bing"), Some(d.join("20261126.jpg")));
        prune(dir.path(), "bing", 2);
        assert!(!d.join("20261124.jpg").exists());
        assert!(d.join("20261125.jpg").exists());
        assert_eq!(latest(dir.path(), "rien"), None);
        assert!(by_id("bing").is_some());
    }
}
