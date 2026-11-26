//! Recherche rapide (lot G) : dossiers indexés, exclusions, onglets, récents…
//!
//! Fichier propre à son lot : les champs s'ajoutent ici, chacun avec une valeur par
//! défaut (`#[serde(default)]` sur la structure). Le raccourci est dans
//! [`super::Hotkeys::search`].

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchSettings {
    /// Dossiers par défaut retirés de l'index, par clé (`desktop`, `documents`,
    /// `downloads`, `pictures`, `music`, `videos`, `programs`).
    pub disabled_defaults: Vec<String>,
    /// Dossiers indexés en plus des dossiers par défaut.
    pub folders: Vec<PathBuf>,
    /// Exclusions de l'utilisateur, en plus de celles de Tidy : un nom de dossier ou de
    /// fichier (`build`), ou un chemin complet (tout ce qui est dessous).
    pub exclusions: Vec<String>,
    /// Indexer aussi les éléments cachés (nom en `.`, attribut « caché » de Windows).
    pub index_hidden: bool,
    /// Onglets masqués, par clé (`desktop`, `apps`, `documents`…). « Tout » reste.
    pub hidden_tabs: Vec<String>,
    /// Montrer les éléments ouverts récemment quand le champ est vide.
    pub show_recents: bool,
    /// Garder le lanceur en mémoire, fenêtre cachée, pour qu'il se rouvre aussitôt.
    pub keep_in_background: bool,
    /// macOS : proposer « Plus de résultats » (Spotlight) sous la liste.
    pub more_results: bool,
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            disabled_defaults: Vec::new(),
            folders: Vec::new(),
            exclusions: Vec::new(),
            index_hidden: false,
            hidden_tabs: Vec::new(),
            show_recents: true,
            keep_in_background: true,
            more_results: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_json_gives_defaults() {
        let s: SearchSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, SearchSettings::default());
        assert!(s.show_recents && s.keep_in_background);
    }
}
