//! Fonds d'écran animés (`tidy --wallpaper`, lot J) : processus lancé et surveillé
//! par l'agent quand un fond vidéo ou une playlist est actif.

use anyhow::Result;
use tidy_core::Store;

pub fn run(store: Store) -> Result<()> {
    let _ = store;
    println!("Fonds d'écran : à venir.");
    Ok(())
}
