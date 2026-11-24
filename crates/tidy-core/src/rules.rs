//! Règles de rangement : quel élément va dans quelle boîte.
//!
//! Chaque règle vise une boîte et porte un [`Criterion`]. La règle de plus haute
//! priorité qui correspond l'emporte ; à priorité égale, la première de la liste. Les
//! critères intégrés sont évalués ici ; les autres passent par un [`RuleMatcher`]
//! enregistré dans un [`MatcherRegistry`] (point d'extension : un nouveau critère ne
//! touche pas cette crate).

use std::collections::BTreeMap;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::category::{Category, extension_of};
use crate::model::BoxId;

/// Ce que le moteur sait d'un élément du bureau au moment de le ranger.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemInfo {
    /// Nom dans `~/Desktop`.
    pub name: String,
    pub is_dir: bool,
    /// Catégorie déjà déterminée (UTI, contenu des liens…) par l'appelant.
    pub category: Category,
    pub size: u64,
    pub modified: Option<Timestamp>,
    /// Type uniforme de macOS (`public.jpeg`…), s'il est connu.
    pub uti: Option<String>,
}

/// Critère d'une règle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Criterion {
    /// Une catégorie (Documents, Images…).
    Category { category: Category },
    /// Une liste d'extensions (sans point, en minuscules).
    Extensions { extensions: Vec<String> },
    /// Un motif de nom, avec `*` et `?` (insensible à la casse).
    NamePattern { pattern: String },
    /// Modifié depuis moins de `days` jours.
    ModifiedWithin { days: u32 },
    /// Plus gros que `bytes`.
    LargerThan { bytes: u64 },
    /// Critère fourni par un [`RuleMatcher`] enregistré sous `matcher`.
    Custom {
        matcher: String,
        #[serde(default)]
        params: serde_json::Value,
    },
}

/// Une règle de rangement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub id: Uuid,
    pub target: BoxId,
    pub criterion: Criterion,
    /// Plus grand = évalué d'abord.
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "enabled")]
    pub enabled: bool,
}

fn enabled() -> bool {
    true
}

impl Rule {
    pub fn new(target: BoxId, criterion: Criterion) -> Self {
        Self {
            id: Uuid::new_v4(),
            target,
            criterion,
            priority: 0,
            enabled: true,
        }
    }

    pub fn category(target: BoxId, category: Category) -> Self {
        Self::new(target, Criterion::Category { category })
    }
}

/// Critère de règle externe (point d'extension).
pub trait RuleMatcher: Send + Sync {
    /// Identifiant stable, enregistré dans les règles (`Criterion::Custom::matcher`).
    fn id(&self) -> &'static str;
    /// L'élément correspond-il, avec ces paramètres ?
    fn matches(&self, item: &ItemInfo, params: &serde_json::Value, now: Timestamp) -> bool;
}

/// Critères externes connus.
#[derive(Default)]
pub struct MatcherRegistry {
    matchers: BTreeMap<&'static str, Box<dyn RuleMatcher>>,
}

impl MatcherRegistry {
    pub fn register(&mut self, matcher: Box<dyn RuleMatcher>) {
        self.matchers.insert(matcher.id(), matcher);
    }

    pub fn get(&self, id: &str) -> Option<&dyn RuleMatcher> {
        self.matchers.get(id).map(|m| m.as_ref())
    }
}

impl Criterion {
    pub fn matches(&self, item: &ItemInfo, registry: &MatcherRegistry, now: Timestamp) -> bool {
        match self {
            Criterion::Category { category } => item.category == *category,
            Criterion::Extensions { extensions } => extension_of(&item.name)
                .is_some_and(|ext| extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext))),
            Criterion::NamePattern { pattern } => wildcard_match(pattern, &item.name),
            Criterion::ModifiedWithin { days } => item
                .modified
                .is_some_and(|m| now.duration_since(m).as_secs() <= i64::from(*days) * 86_400),
            Criterion::LargerThan { bytes } => item.size > *bytes,
            Criterion::Custom { matcher, params } => registry
                .get(matcher)
                .is_some_and(|m| m.matches(item, params, now)),
        }
    }
}

/// Boîte cible d'un élément : la règle active de plus haute priorité qui correspond.
pub fn target_for(
    rules: &[Rule],
    item: &ItemInfo,
    registry: &MatcherRegistry,
    now: Timestamp,
) -> Option<BoxId> {
    let mut best: Option<&Rule> = None;
    for rule in rules.iter().filter(|r| r.enabled) {
        if best.is_some_and(|b| b.priority >= rule.priority) {
            continue;
        }
        if rule.criterion.matches(item, registry, now) {
            best = Some(rule);
        }
    }
    best.map(|r| r.target)
}

/// Motif simple : `*` (n'importe quelle suite), `?` (un caractère), insensible à la
/// casse.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, category: Category) -> ItemInfo {
        ItemInfo {
            name: name.into(),
            is_dir: false,
            category,
            size: 10,
            modified: None,
            uti: None,
        }
    }

    #[test]
    fn wildcards() {
        assert!(wildcard_match("facture*", "Facture 2026.pdf"));
        assert!(wildcard_match("*.pdf", "a.PDF"));
        assert!(wildcard_match("img_????.jpg", "IMG_1234.jpg"));
        assert!(!wildcard_match("img_????.jpg", "IMG_12345.jpg"));
        assert!(wildcard_match("*", ""));
    }

    #[test]
    fn highest_priority_wins_then_first() {
        let docs = BoxId::new();
        let invoices = BoxId::new();
        let other = BoxId::new();
        let mut invoice_rule = Rule::new(
            invoices,
            Criterion::NamePattern {
                pattern: "facture*".into(),
            },
        );
        invoice_rule.priority = 10;
        let rules = vec![
            Rule::category(docs, Category::Document),
            invoice_rule,
            Rule::category(other, Category::Document),
        ];
        let reg = MatcherRegistry::default();
        let now = Timestamp::now();
        assert_eq!(
            target_for(
                &rules,
                &item("Facture mars.pdf", Category::Document),
                &reg,
                now
            ),
            Some(invoices)
        );
        assert_eq!(
            target_for(&rules, &item("Rapport.pdf", Category::Document), &reg, now),
            Some(docs)
        );
        assert_eq!(
            target_for(&rules, &item("photo.jpg", Category::Image), &reg, now),
            None
        );
    }

    struct Even;
    impl RuleMatcher for Even {
        fn id(&self) -> &'static str {
            "even_size"
        }
        fn matches(&self, item: &ItemInfo, _: &serde_json::Value, _: Timestamp) -> bool {
            item.size.is_multiple_of(2)
        }
    }

    #[test]
    fn custom_matchers_come_from_the_registry() {
        let target = BoxId::new();
        let rules = vec![Rule::new(
            target,
            Criterion::Custom {
                matcher: "even_size".into(),
                params: serde_json::Value::Null,
            },
        )];
        let now = Timestamp::now();
        let empty = MatcherRegistry::default();
        assert_eq!(
            target_for(&rules, &item("a", Category::Other), &empty, now),
            None
        );
        let mut reg = MatcherRegistry::default();
        reg.register(Box::new(Even));
        assert_eq!(
            target_for(&rules, &item("a", Category::Other), &reg, now),
            Some(target)
        );
    }
}
