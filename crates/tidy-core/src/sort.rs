//! Tri des éléments d'une boîte : tri naturel des noms (« fichier 2 » avant
//! « fichier 10 »), par type, date ou taille, et regroupement par date (aujourd'hui,
//! hier, cette semaine, ce mois-ci, plus ancien).

use std::cmp::Ordering;

use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;

use crate::model::{SortKey, SortOrder};
use crate::rules::ItemInfo;

/// Trie `items` selon `order`. `SortKey::Manual` : ordre laissé tel quel (même avec le
/// regroupement par date). Avec `order.group_by_date`, les éléments sont d'abord
/// rassemblés par [`DateGroup`] (le plus récent en tête), puis triés dans chaque groupe.
pub fn sort_items(items: &mut [ItemInfo], order: SortOrder) {
    match order.key {
        SortKey::Manual => return,
        SortKey::Name => items.sort_by(|a, b| natural_cmp(&a.name, &b.name)),
        SortKey::Kind => items.sort_by(|a, b| {
            a.category
                .cmp(&b.category)
                .then_with(|| natural_cmp(&a.name, &b.name))
        }),
        SortKey::Modified => items.sort_by(|a, b| {
            a.modified
                .cmp(&b.modified)
                .then_with(|| natural_cmp(&a.name, &b.name))
        }),
        SortKey::Size => items.sort_by(|a, b| {
            a.size
                .cmp(&b.size)
                .then_with(|| natural_cmp(&a.name, &b.name))
        }),
    }
    if order.descending {
        items.reverse();
    }
    if order.group_by_date {
        let now = Timestamp::now();
        let tz = TimeZone::system();
        // Tri stable : l'ordre choisi est gardé dans chaque groupe.
        items.sort_by_key(|i| date_group_in(i.modified, now, &tz));
    }
}

/// Comparaison « naturelle » de deux noms : sans tenir compte de la casse, les suites de
/// chiffres comparées comme des nombres (« 2 » avant « 10 », « 007 » après « 7 » à
/// égalité de valeur). À égalité, l'ordre binaire départage (tri stable et total).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut x = a.chars().peekable();
    let mut y = b.chars().peekable();
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let n = take_digits(&mut x);
                let m = take_digits(&mut y);
                let (n_trim, m_trim) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                let order = n_trim
                    .len()
                    .cmp(&m_trim.len())
                    .then_with(|| n_trim.cmp(m_trim))
                    .then_with(|| n.len().cmp(&m.len()));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(c), Some(d)) => {
                let order = c.to_lowercase().cmp(d.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                x.next();
                y.next();
            }
        }
    }
}

fn take_digits(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut digits = String::new();
    while let Some(c) = it.next_if(char::is_ascii_digit) {
        digits.push(c);
    }
    digits
}

/// Groupe de dates pour le regroupement d'une boîte, du plus récent au plus ancien.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DateGroup {
    Today,
    Yesterday,
    /// Depuis lundi (hors aujourd'hui et hier).
    ThisWeek,
    /// Depuis le 1er du mois (hors cette semaine).
    ThisMonth,
    Older,
}

impl DateGroup {
    pub const ALL: [DateGroup; 5] = [
        DateGroup::Today,
        DateGroup::Yesterday,
        DateGroup::ThisWeek,
        DateGroup::ThisMonth,
        DateGroup::Older,
    ];

    /// Clé de traduction du titre du groupe.
    pub fn key(self) -> &'static str {
        match self {
            DateGroup::Today => "organize.group_today",
            DateGroup::Yesterday => "organize.group_yesterday",
            DateGroup::ThisWeek => "organize.group_this_week",
            DateGroup::ThisMonth => "organize.group_this_month",
            DateGroup::Older => "organize.group_older",
        }
    }
}

/// Groupe de date d'un élément modifié à `modified` (fuseau horaire du système). Sans
/// date : « Plus ancien ». Une date dans le futur compte pour aujourd'hui.
pub fn date_group(modified: impl Into<Option<Timestamp>>, now: Timestamp) -> DateGroup {
    date_group_in(modified, now, &TimeZone::system())
}

/// Comme [`date_group`], dans le fuseau `tz`.
pub fn date_group_in(
    modified: impl Into<Option<Timestamp>>,
    now: Timestamp,
    tz: &TimeZone,
) -> DateGroup {
    let Some(modified) = modified.into() else {
        return DateGroup::Older;
    };
    let today = now.to_zoned_datetime(tz.clone()).date();
    let day = modified.to_zoned_datetime(tz.clone()).date();
    group_of_day(day, today)
}

fn group_of_day(day: Date, today: Date) -> DateGroup {
    if day >= today {
        return DateGroup::Today;
    }
    if today.yesterday().is_ok_and(|y| y == day) {
        return DateGroup::Yesterday;
    }
    let monday = today
        .checked_sub(jiff::Span::new().days(today.weekday().to_monday_zero_offset()))
        .unwrap_or(today);
    if day >= monday {
        return DateGroup::ThisWeek;
    }
    if day >= today.first_of_month() {
        return DateGroup::ThisMonth;
    }
    DateGroup::Older
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::category::Category;
    use jiff::civil::date;

    fn item(name: &str, size: u64) -> ItemInfo {
        ItemInfo {
            name: name.into(),
            is_dir: false,
            category: Category::Other,
            size,
            modified: None,
            uti: None,
        }
    }

    fn names(items: &[ItemInfo]) -> Vec<&str> {
        items.iter().map(|i| i.name.as_str()).collect()
    }

    #[test]
    fn by_name_and_size() {
        let mut items = vec![item("b", 1), item("A", 3), item("c", 2)];
        sort_items(
            &mut items,
            SortOrder {
                key: SortKey::Name,
                ..Default::default()
            },
        );
        assert_eq!(names(&items), ["A", "b", "c"]);
        sort_items(
            &mut items,
            SortOrder {
                key: SortKey::Size,
                descending: true,
                ..Default::default()
            },
        );
        assert_eq!(items.iter().map(|i| i.size).collect::<Vec<_>>(), [3, 2, 1]);
    }

    #[test]
    fn natural_order() {
        let mut items = vec![
            item("photo 10.jpg", 0),
            item("Photo 2.jpg", 0),
            item("photo 1.jpg", 0),
            item("photo 02.jpg", 0),
            item("photo.jpg", 0),
            item("Zèbre", 0),
            item("abc", 0),
        ];
        sort_items(
            &mut items,
            SortOrder {
                key: SortKey::Name,
                ..Default::default()
            },
        );
        assert_eq!(
            names(&items),
            [
                "abc",
                "photo 1.jpg",
                "Photo 2.jpg",
                "photo 02.jpg",
                "photo 10.jpg",
                "photo.jpg",
                "Zèbre"
            ]
        );
        assert_eq!(natural_cmp("a", "a"), Ordering::Equal);
        assert_eq!(natural_cmp("v1.10", "v1.9"), Ordering::Greater);
        assert_eq!(
            natural_cmp("x99999999999999999999999", "x100000000000000000000000"),
            Ordering::Less,
            "pas de dépassement sur les grands nombres"
        );
    }

    #[test]
    fn manual_order_is_kept() {
        let mut items = vec![item("b", 0), item("a", 0)];
        sort_items(
            &mut items,
            SortOrder {
                key: SortKey::Manual,
                group_by_date: true,
                descending: true,
            },
        );
        assert_eq!(names(&items), ["b", "a"]);
    }

    #[test]
    fn date_groups() {
        // Mercredi 18 novembre 2026.
        let today = date(2026, 11, 18);
        assert_eq!(group_of_day(today, today), DateGroup::Today);
        assert_eq!(group_of_day(date(2026, 11, 19), today), DateGroup::Today);
        assert_eq!(
            group_of_day(date(2026, 11, 17), today),
            DateGroup::Yesterday
        );
        assert_eq!(group_of_day(date(2026, 11, 16), today), DateGroup::ThisWeek);
        assert_eq!(
            group_of_day(date(2026, 11, 15), today),
            DateGroup::ThisMonth
        );
        assert_eq!(group_of_day(date(2026, 11, 1), today), DateGroup::ThisMonth);
        assert_eq!(group_of_day(date(2026, 10, 31), today), DateGroup::Older);
        // Lundi : hier était dimanche, la semaine précédente.
        let monday = date(2026, 11, 16);
        assert_eq!(
            group_of_day(date(2026, 11, 15), monday),
            DateGroup::Yesterday
        );
        assert_eq!(
            group_of_day(date(2026, 11, 14), monday),
            DateGroup::ThisMonth
        );
        // Semaine à cheval sur deux mois : « cette semaine » l'emporte.
        let tuesday = date(2026, 12, 1);
        assert_eq!(group_of_day(date(2026, 11, 29), tuesday), DateGroup::Older);
        assert_eq!(
            group_of_day(date(2026, 11, 30), tuesday),
            DateGroup::Yesterday
        );
        let thursday = date(2026, 12, 3);
        assert_eq!(
            group_of_day(date(2026, 11, 30), thursday),
            DateGroup::ThisWeek
        );
    }

    #[test]
    fn date_group_uses_the_time_zone() {
        let paris = TimeZone::fixed(jiff::tz::Offset::constant(1));
        let now: Timestamp = "2026-11-18T10:00:00Z".parse().unwrap();
        // 23 h 30 UTC la veille = 0 h 30 à Paris le jour même.
        let late: Timestamp = "2026-11-17T23:30:00Z".parse().unwrap();
        assert_eq!(date_group_in(late, now, &paris), DateGroup::Today);
        assert_eq!(
            date_group_in(late, now, &TimeZone::UTC),
            DateGroup::Yesterday
        );
        assert_eq!(date_group_in(None, now, &paris), DateGroup::Older);
        // La forme courte accepte aussi une date absente (`ItemInfo::modified`).
        let _ = date_group(item("a", 0).modified, now);
    }

    #[test]
    fn grouping_keeps_the_order_inside_each_group() {
        let now = Timestamp::now();
        let old: Timestamp = "2001-01-01T12:00:00Z".parse().unwrap();
        let mut items = vec![
            ItemInfo {
                modified: Some(old),
                ..item("b vieux", 0)
            },
            ItemInfo {
                modified: Some(now),
                ..item("c neuf", 0)
            },
            ItemInfo {
                modified: Some(old),
                ..item("a vieux", 0)
            },
            ItemInfo {
                modified: Some(now),
                ..item("a neuf", 0)
            },
        ];
        sort_items(
            &mut items,
            SortOrder {
                key: SortKey::Name,
                group_by_date: true,
                descending: false,
            },
        );
        assert_eq!(names(&items), ["a neuf", "c neuf", "a vieux", "b vieux"]);
    }
}
