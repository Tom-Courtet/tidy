//! Lanceur de recherche rapide (`tidy --search [texte]`, lot G) : fenêtre ouverte à la
//! demande par l'agent (raccourci) ou par le widget « Search Mini ».

use anyhow::Result;
use tidy_core::Store;

/// Ouvre le lanceur, avec `query` déjà saisi.
pub fn run(store: Store, query: Option<String>) -> Result<()> {
    let _ = (store, query);
    println!("Recherche rapide : à venir.");
    Ok(())
}
