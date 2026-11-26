//! Icônes rangées dans une boîte : masquées du bureau du système en les déplaçant hors
//! de l'écran (Windows, piste 1 ; logique dans `tidy_core::icon_stash`).

use std::path::PathBuf;

use anyhow::Result;
use tidy_core::icon_stash::{self, SyncReport};
use tidy_core::settings::{DesktopMode, Settings};
use tidy_core::store::{desktop_dirs, resolve_desktop_item};
use tidy_core::{DesktopState, Store};

/// Chemins des éléments rangés dans une boîte (bureau de l'utilisateur ou bureau
/// public).
pub fn boxed_paths(state: &DesktopState) -> Vec<PathBuf> {
    let dirs = desktop_dirs();
    state
        .boxes
        .iter()
        .flat_map(|b| b.items())
        .filter_map(|item| resolve_desktop_item(&dirs, &item.0))
        .collect()
}

/// Masque les icônes rangées et rend les autres. `None` : pas disponible sur cet OS.
pub fn sync(store: &Store, extra: &[PathBuf]) -> Option<Result<SyncReport>> {
    let icons = tidy_platform::current().desktop_icon_positions()?;
    // Boîtes désactivées : rien ne les montre, les icônes rangées restent donc sur le
    // bureau du système (sinon les fichiers seraient invisibles).
    let boxes_shown = store.load::<Settings>().effective_desktop_mode() != DesktopMode::Off;
    let mut boxed = if boxes_shown {
        boxed_paths(&store.load())
    } else {
        Vec::new()
    };
    boxed.extend_from_slice(extra);
    Some(icon_stash::sync(store, icons, &boxed))
}

/// Note ce qu'a fait [`sync`] dans le journal ; `quiet` : rien si rien n'a changé.
pub fn log_report(result: Option<Result<SyncReport>>, quiet: bool) {
    match result {
        Some(Ok(r)) => {
            if !(quiet && r.hidden + r.restored == 0 && r.stuck.is_empty()) {
                log::info!(
                    "icônes rangées : {} masquées, {} rendues ({} icônes sur le bureau)",
                    r.hidden,
                    r.restored,
                    r.on_desktop
                );
            }
            if !r.not_found.is_empty() {
                log::info!(
                    "icônes rangées absentes du bureau de l'Explorateur : {}",
                    list(&r.not_found)
                );
            }
            if !r.stuck.is_empty() {
                log::warn!(
                    "icônes rangées restées à l'écran (l'Explorateur refuse de les déplacer) : {}",
                    list(&r.stuck)
                );
            }
        }
        Some(Err(e)) => log::warn!("icônes rangées : {e:#}"),
        None => {}
    }
}

/// Quelques chemins, pour le journal.
fn list(paths: &[PathBuf]) -> String {
    let mut out: Vec<String> = paths
        .iter()
        .take(8)
        .map(|p| p.display().to_string())
        .collect();
    if paths.len() > 8 {
        out.push(format!("… (+{})", paths.len() - 8));
    }
    out.join(" ; ")
}

/// Secours (`tidy --restore-desktop`) : ramène à une place libre visible les icônes
/// restées hors de l'écran sans entrée au journal (Windows). À appeler après
/// [`restore_all`].
pub fn rescue_offscreen(store: &Store) -> usize {
    let Some(icons) = tidy_platform::current().desktop_icon_positions() else {
        return 0;
    };
    match icon_stash::rescue_offscreen(store, icons) {
        Ok(n) => n,
        Err(e) => {
            log::error!("icônes hors de l'écran non ramenées : {e:#}");
            0
        }
    }
}

/// Rend toutes les icônes masquées par Tidy (quitter, désinstaller, secours).
pub fn restore_all(store: &Store) -> usize {
    let Some(icons) = tidy_platform::current().desktop_icon_positions() else {
        return 0;
    };
    match icon_stash::restore_all(store, icons) {
        Ok(n) => n,
        Err(e) => {
            log::error!("icônes rangées non rendues : {e:#} (essayer `tidy --restore-desktop`)");
            0
        }
    }
}
