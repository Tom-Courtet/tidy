//! Icônes rangées dans une boîte : masquées du bureau du système en les déplaçant hors
//! de l'écran (Windows, piste 1 ; logique dans `tidy_core::icon_stash`).

use std::path::PathBuf;

use anyhow::Result;
use tidy_core::icon_stash::{self, SyncReport};
use tidy_core::store::desktop_dir;
use tidy_core::{DesktopState, Store};

/// Chemins des éléments rangés dans une boîte.
pub fn boxed_paths(state: &DesktopState) -> Vec<PathBuf> {
    let Some(desktop) = desktop_dir() else {
        return Vec::new();
    };
    state
        .boxes
        .iter()
        .flat_map(|b| b.items())
        .map(|item| desktop.join(&item.0))
        .collect()
}

/// Masque les icônes rangées et rend les autres. `None` : pas disponible sur cet OS.
pub fn sync(store: &Store, extra: &[PathBuf]) -> Option<Result<SyncReport>> {
    let icons = tidy_platform::current().desktop_icon_positions()?;
    let mut boxed = boxed_paths(&store.load());
    boxed.extend_from_slice(extra);
    Some(icon_stash::sync(store, icons, &boxed))
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
