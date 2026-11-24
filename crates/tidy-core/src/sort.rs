//! Tri des éléments d'une boîte.
//!
//! Lot A (vague 1) : regroupement par date (aujourd'hui, hier, cette semaine…), tri
//! naturel des noms (« fichier 2 » avant « fichier 10 »).

use crate::model::{SortKey, SortOrder};
use crate::rules::ItemInfo;

/// Trie `items` selon `order`. `SortKey::Manual` : ordre laissé tel quel.
pub fn sort_items(items: &mut [ItemInfo], order: SortOrder) {
    match order.key {
        SortKey::Manual => return,
        SortKey::Name => items.sort_by_key(|i| i.name.to_lowercase()),
        SortKey::Kind => items.sort_by(|a, b| {
            (a.category, a.name.to_lowercase()).cmp(&(b.category, b.name.to_lowercase()))
        }),
        SortKey::Modified => items.sort_by_key(|i| i.modified),
        SortKey::Size => items.sort_by_key(|i| i.size),
    }
    if order.descending {
        items.reverse();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::category::Category;

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
        assert_eq!(
            items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
            ["A", "b", "c"]
        );
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
}
