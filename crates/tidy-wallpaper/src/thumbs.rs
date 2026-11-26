//! Miniatures (générées une fois, en cache) et images préparées pour le système.
//!
//! Tout vit dans `<données>/wallpapers/` ([`crate::cache_dir`]) : ce sont des fichiers
//! de Tidy, qu'il peut recréer ou effacer. Les fichiers de l'utilisateur ne sont que
//! lus.
//!
//! - `thumbs/<id>.png` : miniature de la grille ([`THUMB_WIDTH`] × [`THUMB_HEIGHT`]
//!   au plus) ;
//! - `converted/<id>.png` : copie PNG d'une image que le système ne sait pas poser en
//!   fond (WebP) ;
//! - `colors/rrggbb.png` : petite image d'une couleur unie (macOS n'a pas d'autre moyen
//!   de poser une couleur ; Windows s'en sert aussi, pour avoir une couleur par écran).

use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use image::{DynamicImage, ImageFormat, RgbaImage};
use tidy_core::store::write_atomic;
use uuid::Uuid;

use crate::{MediaKind, Wallpaper, cache_dir};

pub const THUMB_WIDTH: u32 = 320;
pub const THUMB_HEIGHT: u32 = 180;

/// Image RGBA décodée (pour l'interface).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub fn thumb_path(data_root: &Path, id: Uuid) -> PathBuf {
    cache_dir(data_root)
        .join("thumbs")
        .join(format!("{id}.png"))
}

fn converted_path(data_root: &Path, id: Uuid) -> PathBuf {
    cache_dir(data_root)
        .join("converted")
        .join(format!("{id}.png"))
}

fn save_png(img: &DynamicImage, dst: &Path) -> Result<()> {
    let mut bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .context("encodage PNG")?;
    write_atomic(dst, &bytes)
}

/// Le système sait-il poser cette image telle quelle ?
fn needs_conversion(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("webp"))
}

/// Fichier à donner au système pour ce fond : l'original, ou sa copie PNG.
pub fn system_image(data_root: &Path, w: &Wallpaper) -> PathBuf {
    if w.kind == MediaKind::Image && needs_conversion(&w.path) {
        converted_path(data_root, w.id)
    } else {
        w.path.clone()
    }
}

/// Prépare la copie PNG si le système en a besoin (une fois).
pub fn prepare_system_image(data_root: &Path, w: &Wallpaper) -> Result<()> {
    let target = system_image(data_root, w);
    if target == w.path || target.is_file() {
        return Ok(());
    }
    let img = image::open(&w.path).with_context(|| format!("lecture de {}", w.path.display()))?;
    save_png(&img, &target)
}

/// Miniature d'une image, si elle n'existe pas déjà.
pub fn make_image_thumb(data_root: &Path, w: &Wallpaper) -> Result<PathBuf> {
    let dst = thumb_path(data_root, w.id);
    if dst.is_file() {
        return Ok(dst);
    }
    let img = image::open(&w.path).with_context(|| format!("lecture de {}", w.path.display()))?;
    save_png(&img.thumbnail(THUMB_WIDTH, THUMB_HEIGHT), &dst)?;
    Ok(dst)
}

/// Miniature à partir d'une image déjà décodée (vignette d'une vidéo donnée par le
/// système).
pub fn save_thumb(data_root: &Path, id: Uuid, image: Rgba) -> Result<PathBuf> {
    let dst = thumb_path(data_root, id);
    let rgba = RgbaImage::from_raw(image.width, image.height, image.pixels)
        .context("image RGBA incohérente")?;
    let img = DynamicImage::ImageRgba8(rgba);
    save_png(&img.thumbnail(THUMB_WIDTH, THUMB_HEIGHT), &dst)?;
    Ok(dst)
}

/// Lit une miniature (ou toute image) en RGBA.
pub fn load(path: &Path) -> Option<Rgba> {
    let img = image::open(path).ok()?.into_rgba8();
    Some(Rgba {
        width: img.width(),
        height: img.height(),
        pixels: img.into_raw(),
    })
}

/// Efface la miniature et la copie préparée d'un fond retiré de la bibliothèque
/// (fichiers de Tidy seulement).
pub fn forget(data_root: &Path, id: Uuid) {
    for path in [thumb_path(data_root, id), converted_path(data_root, id)] {
        if path.starts_with(cache_dir(data_root)) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Image d'une couleur unie (créée au besoin).
pub fn solid_color(data_root: &Path, rgb: [u8; 3]) -> Result<PathBuf> {
    let [r, g, b] = rgb;
    let dst = cache_dir(data_root)
        .join("colors")
        .join(format!("{r:02x}{g:02x}{b:02x}.png"));
    if !dst.is_file() {
        let img = RgbaImage::from_pixel(16, 16, image::Rgba([r, g, b, 255]));
        save_png(&DynamicImage::ImageRgba8(img), &dst)?;
    }
    Ok(dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wallpaper(path: PathBuf) -> Wallpaper {
        Wallpaper {
            id: Uuid::new_v4(),
            kind: MediaKind::of(&path).unwrap(),
            name: "x".into(),
            path,
            category: None,
            favorite: false,
        }
    }

    #[test]
    fn thumbnails_are_made_once_and_small() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("grande.png");
        RgbaImage::from_pixel(1600, 900, image::Rgba([10, 20, 30, 255]))
            .save(&src)
            .unwrap();
        let w = wallpaper(src);
        let thumb = make_image_thumb(dir.path(), &w).unwrap();
        let img = load(&thumb).unwrap();
        assert_eq!((img.width, img.height), (THUMB_WIDTH, THUMB_HEIGHT));
        // Déjà là : pas refaite (on la remplace par un faux fichier pour le voir).
        std::fs::write(&thumb, b"cache").unwrap();
        make_image_thumb(dir.path(), &w).unwrap();
        assert_eq!(std::fs::read(&thumb).unwrap(), b"cache");
        forget(dir.path(), w.id);
        assert!(!thumb.exists());
        assert!(w.path.is_file(), "l'original reste");
    }

    #[test]
    fn webp_is_converted_for_the_system() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("photo.webp");
        RgbaImage::from_pixel(8, 8, image::Rgba([200, 0, 0, 255]))
            .save(&src)
            .unwrap();
        let w = wallpaper(src.clone());
        let target = system_image(dir.path(), &w);
        assert_ne!(target, src);
        prepare_system_image(dir.path(), &w).unwrap();
        assert!(target.is_file());
        let png = wallpaper(dir.path().join("b.png"));
        assert_eq!(system_image(dir.path(), &png), png.path);
    }

    #[test]
    fn solid_colors_are_cached_images() {
        let dir = tempfile::tempdir().unwrap();
        let path = solid_color(dir.path(), [0x7f, 0x3a, 0xed]).unwrap();
        assert!(path.ends_with("7f3aed.png"));
        let img = load(&path).unwrap();
        assert_eq!(&img.pixels[..4], &[0x7f, 0x3a, 0xed, 255]);
    }
}
