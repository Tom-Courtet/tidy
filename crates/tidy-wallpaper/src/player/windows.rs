//! Lecteur Windows : MFPlay (Media Foundation) dans la fenêtre du fond.
//!
//! MFPlay se crée sans rappel : ses opérations sont asynchrones, et [`Player::tick`]
//! rapproche l'état voulu de l'état réel (réglages appliqués une fois la vidéo ouverte,
//! lecture relancée au début quand elle s'arrête en fin de fichier, pause). La reprise
//! en boucle peut donc laisser un court arrêt (au plus l'intervalle entre deux
//! `tick`).

use std::path::Path;

use anyhow::{Context, Result};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tidy_core::settings::wallpaper::Fit;
use windows::Win32::Foundation::{COLORREF, HWND, RECT, SIZE};
use windows::Win32::Media::MediaFoundation::{
    IMFPMediaItem, IMFPMediaPlayer, IMFPMediaPlayerCallback, MF_VERSION, MFP_MEDIAPLAYER_STATE,
    MFP_MEDIAPLAYER_STATE_PAUSED, MFP_MEDIAPLAYER_STATE_PLAYING, MFP_MEDIAPLAYER_STATE_STOPPED,
    MFP_OPTION_NONE, MFPCreateMediaPlayer, MFSTARTUP_FULL, MFShutdown, MFStartup,
    MFVideoARMode_None, MFVideoARMode_PreservePicture, MFVideoNormalizedRect,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::core::{HSTRING, PCWSTR};

use super::VideoOptions;
use crate::thumbs::Rgba;

pub struct Player {
    player: IMFPMediaPlayer,
    hwnd: HWND,
    options: VideoOptions,
    paused: bool,
    /// Réglages (son, ajustement) appliqués à la vidéo ouverte.
    configured: bool,
    /// Dernier champ : Media Foundation n'est arrêté qu'après l'arrêt du lecteur
    /// ([`Drop`] de `Player`) et la libération de son interface (champ `player`).
    _mf: MediaFoundation,
}

/// Media Foundation démarré (`MFStartup`) : arrêté (`MFShutdown`, compté comme le
/// démarrage) quand la valeur disparaît, y compris si la création du lecteur échoue.
struct MediaFoundation;

impl MediaFoundation {
    fn start() -> Result<Self> {
        // SAFETY: démarrage de Media Foundation sur ce thread ; chaque démarrage réussi
        // est équilibré par l'arrêt de `Drop`.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }
            .context("Media Foundation indisponible")?;
        Ok(Self)
    }
}

impl Drop for MediaFoundation {
    fn drop(&mut self) {
        // SAFETY: équilibre le `MFStartup` réussi de `start` ; plus aucun objet Media
        // Foundation de ce lecteur n'est vivant (voir l'ordre des champs de `Player`).
        unsafe {
            let _ = MFShutdown();
        }
    }
}

fn hwnd_of(window: &dyn HasWindowHandle) -> Option<HWND> {
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut _)),
        _ => None,
    }
}

impl Player {
    pub fn new(window: &dyn HasWindowHandle, path: &Path, options: VideoOptions) -> Result<Self> {
        let hwnd = hwnd_of(window).context("fenêtre native introuvable")?;
        // SAFETY: initialisation de COM sur ce thread (celui de la fenêtre) ; des
        // appels répétés sont comptés et sans danger.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let mf = MediaFoundation::start()?;
        let mut player: Option<IMFPMediaPlayer> = None;
        // SAFETY: pas d'adresse (la vidéo est donnée ensuite), pas de rappel ; `hwnd`
        // est une fenêtre vivante de ce thread ; `player` reçoit l'interface créée.
        unsafe {
            MFPCreateMediaPlayer(
                PCWSTR::null(),
                false,
                MFP_OPTION_NONE,
                None::<&IMFPMediaPlayerCallback>,
                Some(hwnd),
                Some(&mut player),
            )
        }
        .context("lecteur MFPlay impossible")?;
        let player = player.context("lecteur MFPlay absent")?;
        let url = HSTRING::from(path.as_os_str());
        let mut item: Option<IMFPMediaItem> = None;
        // SAFETY: `url` vit pendant l'appel ; ouverture synchrone (le fichier est
        // local) ; `item` reçoit l'interface créée.
        unsafe { player.CreateMediaItemFromURL(&url, true, 0, Some(&mut item)) }
            .with_context(|| format!("vidéo illisible : {}", path.display()))?;
        let item = item.context("vidéo illisible")?;
        // SAFETY: `item` vient de ce lecteur.
        unsafe { player.SetMediaItem(&item) }.context("vidéo refusée par le lecteur")?;
        Ok(Self {
            player,
            hwnd,
            options,
            paused: false,
            configured: false,
            _mf: mf,
        })
    }

    fn state(&self) -> Option<MFP_MEDIAPLAYER_STATE> {
        // SAFETY: lecture d'état d'un lecteur vivant.
        unsafe { self.player.GetState() }.ok()
    }

    fn media_ready(&self) -> bool {
        // SAFETY: lecture d'état d'un lecteur vivant.
        unsafe { self.player.GetMediaItem() }.is_ok()
    }

    fn apply_sound(&self) {
        let volume = self.options.volume.clamp(0.0, 1.0);
        // SAFETY: réglages simples d'un lecteur vivant.
        unsafe {
            let _ = self.player.SetMute(self.options.muted);
            let _ = self.player.SetVolume(volume);
        }
    }

    /// Ajustement : « Remplir » montre la partie centrale de la vidéo aux proportions
    /// de la fenêtre ; « Étirer » déforme ; les autres gardent toute l'image.
    fn apply_fit(&self) {
        let [r, g, b] = self.options.background;
        let border = COLORREF(u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16));
        let mut source = MFVideoNormalizedRect {
            left: 0.0,
            top: 0.0,
            right: 1.0,
            bottom: 1.0,
        };
        let mode = match self.options.fit {
            Fit::Stretch => MFVideoARMode_None,
            _ => MFVideoARMode_PreservePicture,
        };
        if self.options.fit == Fit::Fill
            && let (Some(video), Some(window)) = (self.video_size(), self.window_size())
        {
            let [left, top, right, bottom] = super::fill_crop(video, window);
            source = MFVideoNormalizedRect {
                left,
                top,
                right,
                bottom,
            };
        }
        // SAFETY: réglages d'un lecteur vivant ; `source` est une structure locale.
        unsafe {
            let _ = self.player.SetBorderColor(border);
            let _ = self.player.SetAspectRatioMode(mode.0 as u32);
            let _ = self.player.SetVideoSourceRect(&source);
            let _ = self.player.UpdateVideo();
        }
    }

    fn video_size(&self) -> Option<(u32, u32)> {
        let mut size = SIZE::default();
        // SAFETY: `size` est une sortie locale ; le rapport d'aspect n'est pas demandé.
        unsafe { self.player.GetNativeVideoSize(Some(&mut size), None) }.ok()?;
        (size.cx > 0 && size.cy > 0).then_some((size.cx as u32, size.cy as u32))
    }

    fn window_size(&self) -> Option<(u32, u32)> {
        let mut rect = RECT::default();
        // SAFETY: `self.hwnd` est la fenêtre de ce lecteur ; `rect` est local.
        unsafe { GetClientRect(self.hwnd, &mut rect) }.ok()?;
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        (w > 0 && h > 0).then_some((w as u32, h as u32))
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        self.tick();
    }

    pub fn set_options(&mut self, options: VideoOptions) {
        self.options = options;
        if self.configured {
            self.apply_sound();
            self.apply_fit();
        }
    }

    pub fn tick(&mut self) {
        if !self.media_ready() {
            return;
        }
        if !self.configured {
            // Le son d'abord : rien ne doit s'entendre avant d'être coupé.
            self.apply_sound();
            if self.video_size().is_none() {
                return;
            }
            self.apply_fit();
            self.configured = true;
        }
        let state = self.state();
        // SAFETY: commandes simples d'un lecteur vivant.
        unsafe {
            if self.paused {
                if state == Some(MFP_MEDIAPLAYER_STATE_PLAYING) {
                    let _ = self.player.Pause();
                }
            } else if state == Some(MFP_MEDIAPLAYER_STATE_STOPPED) {
                // Fin de la vidéo (ou pas encore lancée) : depuis le début.
                let _ = self.player.Stop();
                let _ = self.player.Play();
            } else if state == Some(MFP_MEDIAPLAYER_STATE_PAUSED) {
                let _ = self.player.Play();
            }
        }
    }

    pub fn resized(&mut self) {
        if self.configured {
            self.apply_fit();
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // SAFETY: arrêt du lecteur avant de relâcher l'interface.
        unsafe {
            let _ = self.player.Shutdown();
        }
    }
}

/// Windows : la vignette des vidéos vient de l'Explorateur (`tidy-platform`).
pub fn poster_frame(_: &Path, _: u32) -> Option<Rgba> {
    None
}
