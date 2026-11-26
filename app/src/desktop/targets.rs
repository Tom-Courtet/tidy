//! Menu ☰ > Règles > « Ranger ici… » : quelles catégories de la page Rangement vont
//! dans cette boîte, et changer la boîte cible d'une catégorie.
//!
//! La boîte cible est un réglage (`settings.organization`, dans `config.json`), pas
//! une donnée du bureau : le bureau modifie le fichier comme le fait la fenêtre
//! principale, puis prévient l'agent (`Request::ReloadSettings`), qui republie
//! `Event::SettingsChanged` à tous.

use tidy_core::BoxId;
use tidy_core::category::Category;
use tidy_core::settings::{CategorySetting, Organization};
use tidy_ui::{icons, t};

/// Icône d'une catégorie (la même que dans la page Rangement).
pub fn icon(category: Category) -> &'static str {
    match category {
        Category::Application => icons::APP,
        Category::Folder => icons::FOLDER,
        Category::Document => icons::DOCUMENT,
        Category::Image => icons::IMAGE,
        Category::Video => icons::VIDEO,
        Category::Music => icons::MUSIC,
        Category::Archive => icons::ARCHIVE,
        Category::Executable => icons::EXECUTABLE,
        Category::WebLink => icons::LINK,
        Category::Game => icons::GAME,
        Category::Other => icons::OTHER,
    }
}

/// Nom traduit d'une catégorie.
pub fn label(category: Category) -> &'static str {
    match category {
        Category::Application => t!("category.application"),
        Category::Folder => t!("category.folder"),
        Category::Document => t!("category.document"),
        Category::Image => t!("category.image"),
        Category::Video => t!("category.video"),
        Category::Music => t!("category.music"),
        Category::Archive => t!("category.archive"),
        Category::Executable => t!("category.executable"),
        Category::WebLink => t!("category.web_link"),
        Category::Game => t!("category.game"),
        Category::Other => t!("category.other"),
    }
}

/// Ce qui est rangé dans une boîte.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Targets {
    /// Catégories dont cette boîte est la cible, dans l'ordre de [`Category::ALL`].
    pub categories: Vec<Category>,
    /// Boîte « fourre-tout » (ce qu'aucune règle ne prend).
    pub catch_all: bool,
}

/// Catégories rangées dans `b`.
pub fn targets_of(org: &Organization, b: BoxId) -> Targets {
    Targets {
        categories: Category::ALL
            .into_iter()
            .filter(|c| org.categories.get(c).and_then(|s| s.target) == Some(b))
            .collect(),
        catch_all: org.catch_all == Some(b),
    }
}

/// Coche ou décoche `b` comme cible de `category` (`None` : le fourre-tout). Cocher
/// prend la catégorie à une autre boîte ; décocher la laisse sans boîte. Un réglage de
/// catégorie revenu aux valeurs par défaut est retiré, comme dans la page Rangement.
pub fn toggle(org: &mut Organization, b: BoxId, category: Option<Category>) {
    let Some(category) = category else {
        org.catch_all = if org.catch_all == Some(b) {
            None
        } else {
            Some(b)
        };
        return;
    };
    let setting = org
        .categories
        .entry(category)
        .or_insert_with(|| CategorySetting {
            target: None,
            extensions: None,
            include_aliases: false,
        });
    setting.target = if setting.target == Some(b) {
        None
    } else {
        Some(b)
    };
    if setting.target.is_none() && setting.extensions.is_none() && !setting.include_aliases {
        org.categories.remove(&category);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_moves_a_category_and_cleans_up() {
        let mut org = Organization::default();
        let (a, b) = (BoxId::new(), BoxId::new());
        toggle(&mut org, a, Some(Category::Image));
        toggle(&mut org, a, Some(Category::Document));
        assert_eq!(
            targets_of(&org, a).categories,
            [Category::Document, Category::Image]
        );
        // Prise par une autre boîte.
        toggle(&mut org, b, Some(Category::Image));
        assert_eq!(targets_of(&org, a).categories, [Category::Document]);
        assert_eq!(targets_of(&org, b).categories, [Category::Image]);
        // Décochée : plus de réglage du tout (rien d'autre n'était modifié).
        toggle(&mut org, a, Some(Category::Document));
        assert!(!org.categories.contains_key(&Category::Document));
    }

    #[test]
    fn custom_extensions_survive_unchecking() {
        let mut org = Organization::default();
        let a = BoxId::new();
        org.categories.insert(
            Category::Image,
            CategorySetting {
                target: Some(a),
                extensions: Some(vec!["png".into()]),
                include_aliases: false,
            },
        );
        toggle(&mut org, a, Some(Category::Image));
        let s = &org.categories[&Category::Image];
        assert_eq!(s.target, None);
        assert_eq!(s.extensions.as_deref(), Some(&["png".to_owned()][..]));
    }

    #[test]
    fn catch_all() {
        let mut org = Organization::default();
        let (a, b) = (BoxId::new(), BoxId::new());
        toggle(&mut org, a, None);
        assert!(targets_of(&org, a).catch_all);
        toggle(&mut org, b, None);
        assert!(!targets_of(&org, a).catch_all);
        assert!(targets_of(&org, b).catch_all);
        toggle(&mut org, b, None);
        assert_eq!(org.catch_all, None);
    }
}
