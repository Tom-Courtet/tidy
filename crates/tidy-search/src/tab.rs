//! Onglets du lanceur : Tout, Bureau, Applis, Documents, Images, Vidéos, Musique,
//! Dossiers, Archives (ceux de l'original).

use tidy_core::category::Category;

/// Onglet du lanceur, qui filtre les résultats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tab {
    #[default]
    All,
    Desktop,
    Apps,
    Documents,
    Images,
    Videos,
    Music,
    Folders,
    Archives,
}

impl Tab {
    pub const ALL: [Tab; 9] = [
        Tab::All,
        Tab::Desktop,
        Tab::Apps,
        Tab::Documents,
        Tab::Images,
        Tab::Videos,
        Tab::Music,
        Tab::Folders,
        Tab::Archives,
    ];

    /// Clé stable (réglages, traductions `search.tab_<clé>`).
    pub fn key(self) -> &'static str {
        match self {
            Tab::All => "all",
            Tab::Desktop => "desktop",
            Tab::Apps => "apps",
            Tab::Documents => "documents",
            Tab::Images => "images",
            Tab::Videos => "videos",
            Tab::Music => "music",
            Tab::Folders => "folders",
            Tab::Archives => "archives",
        }
    }

    pub fn from_key(key: &str) -> Option<Tab> {
        Tab::ALL.into_iter().find(|t| t.key() == key)
    }

    /// Un élément de cette catégorie, sur le bureau ou non, a-t-il sa place ici ?
    pub fn accepts(self, category: Category, on_desktop: bool) -> bool {
        match self {
            Tab::All => true,
            Tab::Desktop => on_desktop,
            Tab::Apps => matches!(category, Category::Application | Category::Game),
            Tab::Documents => category == Category::Document,
            Tab::Images => category == Category::Image,
            Tab::Videos => category == Category::Video,
            Tab::Music => category == Category::Music,
            Tab::Folders => category == Category::Folder,
            Tab::Archives => category == Category::Archive,
        }
    }

    /// Onglets à afficher : « Tout », puis ceux que l'utilisateur n'a pas masqués.
    pub fn visible(hidden: &[String]) -> Vec<Tab> {
        Tab::ALL
            .into_iter()
            .filter(|t| *t == Tab::All || !hidden.iter().any(|h| h == t.key()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_and_filters() {
        for tab in Tab::ALL {
            assert_eq!(Tab::from_key(tab.key()), Some(tab));
        }
        assert!(Tab::Apps.accepts(Category::Application, false));
        assert!(Tab::Apps.accepts(Category::Game, false));
        assert!(!Tab::Apps.accepts(Category::Document, true));
        assert!(Tab::Desktop.accepts(Category::Other, true));
        assert!(!Tab::Desktop.accepts(Category::Other, false));
        let shown = Tab::visible(&["music".into(), "all".into()]);
        assert_eq!(shown.first(), Some(&Tab::All), "« Tout » ne se masque pas");
        assert!(!shown.contains(&Tab::Music));
    }
}
