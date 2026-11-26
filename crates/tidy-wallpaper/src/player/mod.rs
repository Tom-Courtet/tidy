//! Lecteur vidéo natif, dans une fenêtre donnée (processus `tidy --wallpaper`).
//!
//! - Windows : **MFPlay** (Media Foundation) ; le rendu passe par l'EVR, qui confie le
//!   décodage à la carte graphique (DXVA) quand elle le sait.
//! - macOS : **AVPlayerLayer** dans la vue de la fenêtre, `AVPlayerLooper` pour une
//!   boucle sans coupure ; décodage matériel par AVFoundation.
//!
//! Toujours en boucle, muet par défaut. Seul module de la crate où `unsafe` est permis
//! (appels COM et Objective-C), avec `// SAFETY:` à chaque bloc.

use std::path::Path;

use anyhow::Result;
use raw_window_handle::HasWindowHandle;
use tidy_core::settings::wallpaper::Fit;

use crate::thumbs::Rgba;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "macos", windows)))]
mod other;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(not(any(target_os = "macos", windows)))]
use other as imp;
#[cfg(windows)]
use windows as imp;

/// Réglages de lecture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoOptions {
    pub fit: Fit,
    /// Couleur des bandes (ajustement « Ajuster »).
    pub background: [u8; 3],
    /// Volume de 0 à 1.
    pub volume: f32,
    pub muted: bool,
}

impl Default for VideoOptions {
    fn default() -> Self {
        Self {
            fit: Fit::Fill,
            background: [0, 0, 0],
            volume: 0.5,
            muted: true,
        }
    }
}

/// Lecture d'une vidéo en boucle dans une fenêtre.
pub struct Player {
    inner: imp::Player,
}

impl Player {
    /// Commence la lecture de `path` dans `window` (thread principal).
    pub fn new(window: &dyn HasWindowHandle, path: &Path, options: VideoOptions) -> Result<Self> {
        Ok(Self {
            inner: imp::Player::new(window, path, options)?,
        })
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.inner.set_paused(paused);
    }

    pub fn set_options(&mut self, options: VideoOptions) {
        self.inner.set_options(options);
    }

    /// À appeler régulièrement (quelques fois par seconde) : reprise en boucle,
    /// réglages appliqués dès que la vidéo est ouverte (Windows).
    pub fn tick(&mut self) {
        self.inner.tick();
    }

    /// La fenêtre a changé de taille.
    pub fn resized(&mut self) {
        self.inner.resized();
    }
}

/// Première image d'une vidéo, pour la miniature (macOS ; ailleurs, le système donne
/// la vignette par `tidy-platform`).
pub fn poster_frame(path: &Path, max_side: u32) -> Option<Rgba> {
    imp::poster_frame(path, max_side)
}

/// Partie de la vidéo à montrer pour **remplir** une fenêtre sans déformer l'image :
/// rectangle normalisé (gauche, haut, droite, bas), centré.
pub fn fill_crop(video: (u32, u32), window: (u32, u32)) -> [f32; 4] {
    let (vw, vh) = (video.0.max(1) as f32, video.1.max(1) as f32);
    let (ww, wh) = (window.0.max(1) as f32, window.1.max(1) as f32);
    let video_ratio = vw / vh;
    let window_ratio = ww / wh;
    if video_ratio > window_ratio {
        // Vidéo plus large : on coupe à gauche et à droite.
        let visible = window_ratio / video_ratio;
        let margin = (1.0 - visible) / 2.0;
        [margin, 0.0, 1.0 - margin, 1.0]
    } else {
        let visible = video_ratio / window_ratio;
        let margin = (1.0 - visible) / 2.0;
        [0.0, margin, 1.0, 1.0 - margin]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn fill_crops_the_longer_side() {
        // Même rapport : rien à couper.
        assert!(close(
            fill_crop((1920, 1080), (3840, 2160)),
            [0.0, 0.0, 1.0, 1.0]
        ));
        // Vidéo 16:9 sur écran 4:3 : on coupe les côtés.
        let r = fill_crop((1600, 900), (1200, 900));
        assert!(close(r, [0.125, 0.0, 0.875, 1.0]), "{r:?}");
        // Vidéo 4:3 sur écran 16:9 : on coupe en haut et en bas.
        let r = fill_crop((1200, 900), (1600, 900));
        assert!(close(r, [0.0, 0.125, 1.0, 0.875]), "{r:?}");
        // Tailles nulles : pas de division par zéro.
        let r = fill_crop((0, 0), (0, 0));
        assert!(r.iter().all(|x| x.is_finite()));
    }
}
