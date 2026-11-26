//! Autres OS : pas de lecteur vidéo.

use std::path::Path;

use anyhow::{Result, bail};
use raw_window_handle::HasWindowHandle;

use super::VideoOptions;
use crate::thumbs::Rgba;

pub struct Player;

impl Player {
    pub fn new(_: &dyn HasWindowHandle, _: &Path, _: VideoOptions) -> Result<Self> {
        bail!("fonds vidéo : pas disponible sur cet OS")
    }
    pub fn set_paused(&mut self, _: bool) {}
    pub fn set_options(&mut self, _: VideoOptions) {}
    pub fn tick(&mut self) {}
    pub fn resized(&mut self) {}
}

pub fn poster_frame(_: &Path, _: u32) -> Option<Rgba> {
    None
}
