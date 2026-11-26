//! Lecteur macOS : `AVPlayerLayer` posé dans la vue de la fenêtre du fond,
//! `AVQueuePlayer` et `AVPlayerLooper` pour une boucle sans coupure.

use std::path::Path;

use anyhow::{Context, Result, bail};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, Message};
use objc2_app_kit::NSView;
use objc2_av_foundation::{
    AVAssetImageGenerator, AVLayerVideoGravity, AVLayerVideoGravityResize,
    AVLayerVideoGravityResizeAspect, AVLayerVideoGravityResizeAspectFill, AVPlayerItem,
    AVPlayerLayer, AVPlayerLooper, AVQueuePlayer, AVURLAsset,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, kCGColorSpaceSRGB,
};
use objc2_core_media::CMTime;
use objc2_foundation::{NSArray, NSString, NSURL};
use objc2_quartz_core::{CAAutoresizingMask, CALayer};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tidy_core::settings::wallpaper::Fit;

use super::VideoOptions;
use crate::thumbs::Rgba;

pub struct Player {
    player: Retained<AVQueuePlayer>,
    /// Tient la boucle : relâché, la vidéo s'arrêterait en fin de fichier.
    _looper: Retained<AVPlayerLooper>,
    layer: Retained<AVPlayerLayer>,
    host: Retained<CALayer>,
}

fn ns_view(window: &dyn HasWindowHandle) -> Option<Retained<NSView>> {
    let RawWindowHandle::AppKit(h) = window.window_handle().ok()?.as_raw() else {
        return None;
    };
    // SAFETY: winit fournit un pointeur valide vers la NSView de la fenêtre, vivante
    // tant que la fenêtre existe (l'appelant la tient).
    let view: &NSView = unsafe { h.ns_view.cast().as_ref() };
    Some(view.retain())
}

fn gravity(fit: Fit) -> &'static AVLayerVideoGravity {
    // SAFETY: constantes d'AVFoundation, toujours définies une fois le cadre chargé.
    let value = unsafe {
        match fit {
            Fit::Fill => AVLayerVideoGravityResizeAspectFill,
            Fit::Stretch => AVLayerVideoGravityResize,
            Fit::Fit | Fit::Center | Fit::Tile => AVLayerVideoGravityResizeAspect,
        }
    };
    value.expect("constante d'AVFoundation absente")
}

fn file_url(path: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
}

impl Player {
    pub fn new(window: &dyn HasWindowHandle, path: &Path, options: VideoOptions) -> Result<Self> {
        let mtm = MainThreadMarker::new().context("lecteur vidéo hors du thread principal")?;
        if !path.is_file() {
            bail!("vidéo introuvable : {}", path.display());
        }
        let view = ns_view(window).context("vue de la fenêtre introuvable")?;
        view.setWantsLayer(true);
        let host = view.layer().context("vue sans calque")?;
        let url = file_url(path);
        // SAFETY: objets AVFoundation créés sur le thread principal (`mtm`), avec des
        // arguments valides ; le lecteur, la boucle et le calque sont gardés ensemble.
        let (player, looper, layer) = unsafe {
            let item = AVPlayerItem::playerItemWithURL(&url, mtm);
            let player = AVQueuePlayer::queuePlayerWithItems(&NSArray::new(), mtm);
            let looper = AVPlayerLooper::playerLooperWithPlayer_templateItem(&player, &item);
            // Un fond d'écran ne doit pas empêcher l'écran de se mettre en veille.
            player.setPreventsDisplaySleepDuringVideoPlayback(false);
            let layer = AVPlayerLayer::playerLayerWithPlayer(Some(&player));
            (player, looper, layer)
        };
        layer.setFrame(host.bounds());
        layer.setAutoresizingMask(
            CAAutoresizingMask::LayerWidthSizable | CAAutoresizingMask::LayerHeightSizable,
        );
        host.addSublayer(&layer);
        let mut this = Self {
            player,
            _looper: looper,
            layer,
            host,
        };
        this.set_options(options);
        // SAFETY: lecteur vivant, thread principal.
        unsafe { this.player.play() };
        Ok(this)
    }

    pub fn set_paused(&mut self, paused: bool) {
        // SAFETY: lecteur vivant, thread principal.
        unsafe {
            if paused {
                self.player.pause();
            } else {
                self.player.play();
            }
        }
    }

    pub fn set_options(&mut self, options: VideoOptions) {
        // SAFETY: réglages simples d'objets vivants, thread principal.
        unsafe {
            self.player.setMuted(options.muted);
            self.player.setVolume(options.volume.clamp(0.0, 1.0));
            self.layer.setVideoGravity(gravity(options.fit));
        }
    }

    pub fn tick(&mut self) {}

    pub fn resized(&mut self) {
        self.layer.setFrame(self.host.bounds());
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // SAFETY: arrêt du lecteur et retrait du calque, thread principal.
        unsafe { self.player.pause() };
        self.layer.removeFromSuperlayer();
    }
}

/// Première image (à 1 s, ou au plus près) d'une vidéo, `max_side` pixels au plus.
pub fn poster_frame(path: &Path, max_side: u32) -> Option<Rgba> {
    let url = file_url(path);
    let side = f64::from(max_side.max(16));
    // SAFETY: objets AVFoundation créés et utilisés sur ce seul thread ; pas de
    // dictionnaire d'options ; `actualTime` peut être nul.
    #[allow(deprecated)]
    let image: Retained<CGImage> = unsafe {
        let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
        let generator = AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset);
        generator.setAppliesPreferredTrackTransform(true);
        generator.setMaximumSize(CGSize::new(side, side));
        generator
            .copyCGImageAtTime_actualTime_error(CMTime::new(1, 1), std::ptr::null_mut())
            .ok()?
    };
    let width = CGImage::width(Some(&image));
    let height = CGImage::height(Some(&image));
    if width == 0 || height == 0 {
        return None;
    }
    // SAFETY: constante de CoreGraphics, toujours définie.
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
    let mut pixels = vec![0u8; width * height * 4];
    // SAFETY: `pixels` a la place de `height` lignes de `width * 4` octets et survit au
    // contexte (relâché avant la lecture).
    let context = unsafe {
        CGBitmapContextCreate(
            pixels.as_mut_ptr().cast(),
            width,
            height,
            8,
            width * 4,
            Some(&space),
            // Image opaque : octets R, G, B, puis un octet ignoré.
            CGImageAlphaInfo::NoneSkipLast.0,
        )
    }?;
    CGContext::draw_image(
        Some(&context),
        CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(width as f64, height as f64),
        ),
        Some(&image),
    );
    drop(context);
    for px in pixels.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    Some(Rgba {
        width: width as u32,
        height: height as u32,
        pixels,
    })
}
