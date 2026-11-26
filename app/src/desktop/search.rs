//! Recherche dans la barre de titre d'une boîte (`DeskBox::search_in_title`) : le texte
//! tapé filtre les éléments affichés, sans tenir compte de la casse ni des accents
//! (« ete » trouve « Été 2026.jpg », y compris quand le nom est écrit en forme
//! décomposée, comme souvent sous macOS).
//!
//! Le texte cherché n'est pas enregistré : il vit avec la fenêtre (voir
//! [`super::boxwin::BoxUi`]).

use super::items::Section;

/// Champ de recherche ouvert dans la barre de titre.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchEdit {
    pub text: String,
    /// Le champ doit prendre le focus (premier passage).
    pub focus: bool,
}

impl SearchEdit {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            focus: true,
        }
    }

    /// Texte cherché, replié ([`fold`]) ; `None` s'il est vide.
    pub fn query(&self) -> Option<String> {
        let q = fold(self.text.trim());
        (!q.is_empty()).then_some(q)
    }
}

/// Lettre de base d'une lettre accentuée (latin), ou `None`.
fn base_of(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'æ' => "ae",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'ď' | 'đ' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'ĥ' | 'ħ' => "h",
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'ĵ' => "j",
        'ķ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "l",
        'ñ' | 'ń' | 'ņ' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'œ' => "oe",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => "s",
        'ß' => "ss",
        'ţ' | 'ť' | 'ŧ' | 'ț' => "t",
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        _ => return None,
    })
}

/// Texte replié pour comparer : minuscules, sans accents (lettres précomposées et
/// diacritiques combinants), espaces gardées.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        // Diacritiques combinants (forme décomposée : « e » + « ́ »).
        if ('\u{0300}'..='\u{036F}').contains(&c) {
            continue;
        }
        match base_of(c) {
            Some(base) => out.push_str(base),
            None => out.push(c),
        }
    }
    out
}

/// Le nom correspond au texte cherché (déjà replié).
pub fn matches(name: &str, query: &str) -> bool {
    query.is_empty() || fold(name).contains(query)
}

/// Sections réduites aux éléments qui correspondent ; les sections vides disparaissent.
pub fn filter_sections(sections: &[Section], query: &str) -> Vec<Section> {
    sections
        .iter()
        .filter_map(|s| {
            let items: Vec<_> = s
                .items
                .iter()
                .filter(|i| matches(&i.name, query))
                .cloned()
                .collect();
            (!items.is_empty()).then_some(Section {
                group: s.group,
                items,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidy_core::category::Category;
    use tidy_core::rules::ItemInfo;
    use tidy_core::sort::DateGroup;

    fn info(name: &str) -> ItemInfo {
        ItemInfo {
            name: name.into(),
            is_dir: false,
            category: Category::Other,
            size: 0,
            modified: None,
            uti: None,
        }
    }

    #[test]
    fn case_and_accents_are_ignored() {
        assert_eq!(fold("Été À l'Œil"), "ete a l'oeil");
        // Forme décomposée (macOS) : « e » + accent aigu combinant.
        assert_eq!(fold("E\u{301}te\u{301}"), "ete");
        assert!(matches("Été 2026.jpg", &fold("ETE")));
        assert!(matches("Facture électricité.pdf", &fold("electricite")));
        assert!(matches("Straße.txt", &fold("strasse")));
        assert!(!matches("Rapport.pdf", &fold("facture")));
        // Texte vide : tout correspond.
        assert!(matches("n'importe quoi", ""));
    }

    #[test]
    fn query_is_trimmed_and_folded() {
        let mut s = SearchEdit::new();
        assert_eq!(s.query(), None);
        s.text = "   ".into();
        assert_eq!(s.query(), None);
        s.text = "  Été ".into();
        assert_eq!(s.query().as_deref(), Some("ete"));
    }

    #[test]
    fn empty_sections_disappear() {
        let sections = vec![
            Section {
                group: Some(DateGroup::Today),
                items: vec![info("été.jpg"), info("notes.txt")],
            },
            Section {
                group: Some(DateGroup::Older),
                items: vec![info("budget.xlsx")],
            },
        ];
        let out = filter_sections(&sections, &fold("ÉTÉ"));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].group, Some(DateGroup::Today));
        assert_eq!(out[0].items.len(), 1);
        assert_eq!(out[0].items[0].name, "été.jpg");
        assert!(filter_sections(&sections, "zzz").is_empty());
    }
}
