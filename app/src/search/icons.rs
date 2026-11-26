//! Icônes système des résultats, en textures egui, chargées sur un fil à part
//! ([`tidy_platform::DesktopPlatform::file_icon`]). Sans icône système, le lanceur
//! dessine l'icône Lucide de la catégorie.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use egui::{ColorImage, TextureHandle, TextureId, TextureOptions};
use tidy_platform::IconImage;

/// Au-delà, le cache est vidé (les icônes visibles reviennent aussitôt).
const CAPACITY: usize = 400;

enum Slot {
    Pending,
    Missing,
    Ready(TextureHandle),
}

pub struct IconCache {
    entries: HashMap<(PathBuf, u32), Slot>,
    jobs: Sender<(PathBuf, u32)>,
    done: Receiver<((PathBuf, u32), Option<IconImage>)>,
}

impl IconCache {
    /// Lance le fil des icônes ; `ctx` est réveillé à chaque icône prête.
    pub fn new(ctx: egui::Context) -> Self {
        let (jobs, job_rx) = mpsc::channel::<(PathBuf, u32)>();
        let (done_tx, done) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("tidy-icônes-recherche".into())
            .spawn(move || {
                let platform = tidy_platform::current();
                while let Ok((path, px)) = job_rx.recv() {
                    let image = platform.file_icon(&path, px, false);
                    if done_tx.send(((path, px), image)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
        if let Err(e) = spawned {
            log::warn!("fil des icônes : {e}");
        }
        Self {
            entries: HashMap::new(),
            jobs,
            done,
        }
    }

    /// Texture de l'icône de `path`, si elle est prête (la première demande la lance).
    pub fn get(&mut self, path: &Path, px: u32) -> Option<TextureId> {
        let key = (path.to_path_buf(), px);
        if let Some(slot) = self.entries.get(&key) {
            return match slot {
                Slot::Ready(t) => Some(t.id()),
                Slot::Pending | Slot::Missing => None,
            };
        }
        if self.entries.len() >= CAPACITY {
            self.clear();
        }
        let slot = if self.jobs.send(key.clone()).is_ok() {
            Slot::Pending
        } else {
            Slot::Missing
        };
        self.entries.insert(key, slot);
        None
    }

    /// Charge les icônes arrivées.
    pub fn receive(&mut self, ctx: &egui::Context) {
        while let Ok((key, image)) = self.done.try_recv() {
            let Some(slot) = self.entries.get_mut(&key) else {
                continue;
            };
            *slot = match image.and_then(to_color_image) {
                Some(image) => Slot::Ready(ctx.load_texture(
                    format!("recherche {}", key.0.display()),
                    image,
                    TextureOptions::LINEAR,
                )),
                None => Slot::Missing,
            };
        }
    }

    /// Libère toutes les textures (fenêtre cachée).
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

fn to_color_image(icon: IconImage) -> Option<ColorImage> {
    let (w, h) = (icon.width as usize, icon.height as usize);
    if w == 0 || h == 0 || icon.rgba.len() != w * h * 4 {
        return None;
    }
    Some(ColorImage::from_rgba_unmultiplied([w, h], &icon.rgba))
}
