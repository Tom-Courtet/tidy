//! Icônes système des éléments, en textures egui.
//!
//! Les icônes sont demandées à [`tidy_platform::DesktopPlatform::file_icon`] sur un fil
//! à part (le système peut être lent, surtout pour les miniatures), puis chargées en
//! texture au passage suivant. Le cache est borné : les icônes les moins récemment
//! affichées sont libérées, et toutes celles qu'on n'a pas vues depuis un moment aussi
//! (boîte enroulée, élément parti). Sans icône système, l'appelant dessine un repli
//! (icône Lucide de la catégorie).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use egui::{ColorImage, TextureHandle, TextureId, TextureOptions};
use tidy_platform::IconImage;

/// Nombre maximal d'icônes gardées en texture.
const CAPACITY: usize = 160;
/// Une icône qu'on n'a pas affichée depuis ce délai est libérée.
const IDLE: Duration = Duration::from_secs(90);

type Key = (PathBuf, u32);

enum Slot {
    /// Demandée au fil des icônes.
    Pending,
    /// Le système n'en a pas : repli.
    Missing,
    Ready(TextureHandle),
}

struct Entry {
    slot: Slot,
    used: Instant,
}

struct Job {
    path: PathBuf,
    px: u32,
    thumbnail: bool,
}

struct Done {
    key: Key,
    image: Option<IconImage>,
}

pub struct IconCache {
    entries: HashMap<Key, Entry>,
    jobs: Sender<Job>,
    done: Receiver<Done>,
    last_sweep: Instant,
}

impl IconCache {
    /// Lance le fil des icônes ; `ctx` est réveillé à chaque icône prête.
    pub fn new(ctx: egui::Context) -> Self {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (done_tx, done) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("tidy-icônes".into())
            .spawn(move || {
                let platform = tidy_platform::current();
                while let Ok(job) = job_rx.recv() {
                    let image = platform.file_icon(&job.path, job.px, job.thumbnail);
                    if done_tx
                        .send(Done {
                            key: (job.path, job.px),
                            image,
                        })
                        .is_err()
                    {
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
            last_sweep: Instant::now(),
        }
    }

    /// Texture de l'icône de `path` à `px` pixels, si elle est prête. La première
    /// demande lance le chargement ; `None` en attendant ou sans icône système.
    pub fn get(&mut self, path: &Path, px: u32, thumbnail: bool) -> Option<TextureId> {
        let key = (path.to_path_buf(), px);
        let now = Instant::now();
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.used = now;
            return match &entry.slot {
                Slot::Ready(t) => Some(t.id()),
                Slot::Pending | Slot::Missing => None,
            };
        }
        let sent = self.jobs.send(Job {
            path: key.0.clone(),
            px,
            thumbnail,
        });
        let slot = if sent.is_ok() {
            Slot::Pending
        } else {
            Slot::Missing
        };
        self.entries.insert(key, Entry { slot, used: now });
        None
    }

    /// Charge les icônes arrivées. Rend `true` s'il y en a eu.
    pub fn receive(&mut self, ctx: &egui::Context) -> bool {
        let mut any = false;
        while let Ok(done) = self.done.try_recv() {
            any = true;
            let Some(entry) = self.entries.get_mut(&done.key) else {
                continue;
            };
            entry.slot = match done.image.and_then(to_color_image) {
                Some(image) => {
                    let name = format!("icône {}", done.key.0.display());
                    Slot::Ready(ctx.load_texture(name, image, TextureOptions::LINEAR))
                }
                None => Slot::Missing,
            };
        }
        any
    }

    /// Oublie les icônes de `path` (renommé, modifié).
    pub fn forget(&mut self, path: &Path) {
        self.entries.retain(|(p, _), _| p != path);
    }

    /// Libère les textures en trop ou inutilisées depuis longtemps.
    pub fn sweep(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_sweep) < Duration::from_secs(5)
            && self.entries.len() <= CAPACITY
        {
            return;
        }
        self.last_sweep = now;
        let ages: Vec<(Key, Duration)> = self
            .entries
            .iter()
            .filter(|(_, e)| !matches!(e.slot, Slot::Pending))
            .map(|(k, e)| (k.clone(), now.duration_since(e.used)))
            .collect();
        let evicted = evictions(ages, CAPACITY, IDLE);
        if !evicted.is_empty() {
            log::debug!("{} icônes libérées", evicted.len());
        }
        for key in evicted {
            self.entries.remove(&key);
        }
    }
}

/// Entrées à libérer : celles inutilisées depuis plus de `idle`, puis les plus
/// anciennes jusqu'à revenir à `capacity`.
fn evictions<K: Clone>(mut ages: Vec<(K, Duration)>, capacity: usize, idle: Duration) -> Vec<K> {
    ages.sort_by_key(|(_, age)| std::cmp::Reverse(*age));
    let over = ages.len().saturating_sub(capacity);
    ages.iter()
        .enumerate()
        .filter(|(i, (_, age))| *i < over || *age > idle)
        .map(|(_, (k, _))| k.clone())
        .collect()
}

fn to_color_image(icon: IconImage) -> Option<ColorImage> {
    let (w, h) = (icon.width as usize, icon.height as usize);
    if w == 0 || h == 0 || icon.rgba.len() != w * h * 4 {
        return None;
    }
    Some(ColorImage::from_rgba_unmultiplied([w, h], &icon.rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_idle_then_oldest() {
        let s = Duration::from_secs;
        let ages = vec![("a", s(1)), ("b", s(200)), ("c", s(5)), ("d", s(10))];
        // Capacité suffisante : seule l'icône inactive part.
        assert_eq!(evictions(ages.clone(), 10, s(90)), ["b"]);
        // Capacité 2 : les deux plus anciennes partent.
        let mut out = evictions(ages, 2, s(90));
        out.sort_unstable();
        assert_eq!(out, ["b", "d"]);
    }

    #[test]
    fn bad_images_are_refused() {
        let bad = IconImage {
            width: 2,
            height: 2,
            rgba: vec![0; 3],
        };
        assert!(to_color_image(bad).is_none());
        let ok = IconImage {
            width: 1,
            height: 1,
            rgba: vec![1, 2, 3, 4],
        };
        assert_eq!(to_color_image(ok).unwrap().size, [1, 1]);
    }
}
