//! Grille libre d'une boîte : un élément peut occuper une case précise, avec des cases
//! vides entre les éléments, comme les icônes du bureau quand la disposition
//! automatique est coupée.
//!
//! Les cases choisies sont notées par onglet ([`crate::model::Tab::cells`]). Un élément
//! sans case (nouveau venu, ou case hors de la largeur actuelle) prend la première case
//! libre, ligne par ligne. Rétrécir une boîte ne perd rien : un élément dont la colonne
//! n'existe plus est affiché ailleurs, et retrouve sa case quand la boîte s'élargit.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::model::ItemRef;

/// Une case de la grille (colonne, ligne), à partir de 0 en haut à gauche.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Cell {
    pub col: u32,
    pub row: u32,
}

impl Cell {
    pub fn new(col: u32, row: u32) -> Self {
        Self { col, row }
    }

    /// Rang de la case en lisant ligne par ligne.
    fn index(self, columns: u32) -> u64 {
        u64::from(self.row) * u64::from(columns) + u64::from(self.col)
    }
}

/// Première case libre à partir du rang `from` (ligne par ligne).
fn first_free(taken: &HashSet<Cell>, columns: u32, from: u64) -> Cell {
    let mut i = from;
    loop {
        let cell = Cell::new(
            (i % u64::from(columns)) as u32,
            (i / u64::from(columns)) as u32,
        );
        if !taken.contains(&cell) {
            return cell;
        }
        i += 1;
    }
}

/// Case libre la plus proche de `target` (à distance égale : la plus haute, puis la
/// plus à gauche).
fn nearest_free(taken: &HashSet<Cell>, columns: u32, target: Cell) -> Cell {
    let target = Cell::new(target.col.min(columns - 1), target.row);
    if !taken.contains(&target) {
        return target;
    }
    let mut best: Option<(u64, Cell)> = None;
    // Une case libre existe forcément à moins de `taken.len() + 1` lignes.
    let last_row = target.row + taken.len() as u32 + 1;
    for row in 0..=last_row {
        for col in 0..columns {
            let cell = Cell::new(col, row);
            if taken.contains(&cell) {
                continue;
            }
            let dc = i64::from(col) - i64::from(target.col);
            let dr = i64::from(row) - i64::from(target.row);
            let d = (dc * dc + dr * dr) as u64;
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, cell));
            }
        }
    }
    best.map_or(target, |(_, c)| c)
}

/// Case de chaque élément de `items` (même ordre) pour `columns` colonnes. Un élément
/// garde sa case notée si elle tient dans la largeur et n'est pas déjà prise ; les
/// autres prennent les premières cases libres.
pub fn layout(items: &[&str], cells: &BTreeMap<ItemRef, Cell>, columns: usize) -> Vec<Cell> {
    let columns = columns.max(1) as u32;
    let mut taken = HashSet::new();
    let mut out: Vec<Option<Cell>> = items
        .iter()
        .map(|name| {
            let cell = *cells.get(&ItemRef((*name).to_owned()))?;
            (cell.col < columns && taken.insert(cell)).then_some(cell)
        })
        .collect();
    for slot in out.iter_mut().filter(|s| s.is_none()) {
        let cell = first_free(&taken, columns, 0);
        taken.insert(cell);
        *slot = Some(cell);
    }
    out.into_iter().flatten().collect()
}

/// Nombre de lignes occupées par `layout`.
pub fn rows(layout: &[Cell]) -> u32 {
    layout.iter().map(|c| c.row + 1).max().unwrap_or(0)
}

/// Pose `moved` à partir de `target` : le premier sur la case visée (ou la case libre
/// la plus proche), les suivants sur les cases libres qui suivent. `shown` : les
/// éléments affichés dans l'onglet, dans l'ordre ; leur place actuelle est d'abord
/// figée pour que rien d'autre ne bouge.
pub fn place(
    cells: &mut BTreeMap<ItemRef, Cell>,
    shown: &[&str],
    columns: usize,
    moved: &[String],
    target: Cell,
) {
    let columns_u = columns.max(1) as u32;
    let current = layout(shown, cells, columns);
    let mut taken = HashSet::new();
    for (name, cell) in shown.iter().zip(current) {
        if moved.iter().any(|m| m == name) {
            continue;
        }
        cells.insert(ItemRef((*name).to_owned()), cell);
        taken.insert(cell);
    }
    let mut next = None;
    for name in moved {
        let cell = match next {
            None => nearest_free(&taken, columns_u, target),
            Some(from) => first_free(&taken, columns_u, from),
        };
        taken.insert(cell);
        next = Some(cell.index(columns_u) + 1);
        cells.insert(ItemRef(name.clone()), cell);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(list: &[(&str, u32, u32)]) -> BTreeMap<ItemRef, Cell> {
        list.iter()
            .map(|(n, c, r)| (ItemRef((*n).into()), Cell::new(*c, *r)))
            .collect()
    }

    fn at(c: u32, r: u32) -> Cell {
        Cell::new(c, r)
    }

    #[test]
    fn items_without_a_cell_fill_the_gaps() {
        let noted = cells(&[("b", 2, 1)]);
        let l = layout(&["a", "b", "c", "d"], &noted, 3);
        assert_eq!(l, [at(0, 0), at(2, 1), at(1, 0), at(2, 0)]);
        assert_eq!(rows(&l), 2);
    }

    #[test]
    fn a_column_that_no_longer_fits_is_shown_elsewhere_then_comes_back() {
        let noted = cells(&[("a", 4, 0)]);
        assert_eq!(layout(&["a", "b"], &noted, 3), [at(0, 0), at(1, 0)]);
        assert_eq!(layout(&["a", "b"], &noted, 5), [at(4, 0), at(0, 0)]);
    }

    #[test]
    fn the_same_cell_twice_goes_to_the_first() {
        let noted = cells(&[("a", 1, 0), ("b", 1, 0)]);
        assert_eq!(layout(&["a", "b"], &noted, 3), [at(1, 0), at(0, 0)]);
    }

    #[test]
    fn moving_leaves_a_gap_and_freezes_the_others() {
        let mut noted = BTreeMap::new();
        place(&mut noted, &["a", "b", "c"], 3, &["a".into()], at(1, 2));
        // « a » est en bas, les autres n'ont pas bougé.
        assert_eq!(
            layout(&["a", "b", "c"], &noted, 3),
            [at(1, 2), at(1, 0), at(2, 0)]
        );
        // Un nouvel élément prend la case laissée libre.
        assert_eq!(layout(&["a", "b", "c", "d"], &noted, 3)[3], at(0, 0));
    }

    #[test]
    fn dropping_on_a_taken_cell_uses_the_nearest_free_one() {
        let mut noted = BTreeMap::new();
        place(&mut noted, &["a", "b", "c"], 3, &["c".into()], at(0, 0));
        // (0,0) est prise par « a » : la plus proche libre est (0,1).
        assert_eq!(noted[&ItemRef("c".into())], at(0, 1));
        // Au-delà de la largeur : dernière colonne.
        place(&mut noted, &["a", "b", "c"], 3, &["a".into()], at(9, 3));
        assert_eq!(noted[&ItemRef("a".into())], at(2, 3));
    }

    #[test]
    fn several_items_follow_the_first() {
        let mut noted = BTreeMap::new();
        place(
            &mut noted,
            &["a", "b", "c", "d"],
            3,
            &["c".into(), "d".into()],
            at(2, 1),
        );
        let l = layout(&["a", "b", "c", "d"], &noted, 3);
        assert_eq!(l, [at(0, 0), at(1, 0), at(2, 1), at(0, 2)]);
    }

    #[test]
    fn an_item_coming_from_elsewhere_lands_where_dropped() {
        let mut noted = BTreeMap::new();
        place(&mut noted, &["a"], 4, &["x".into()], at(3, 2));
        assert_eq!(layout(&["a", "x"], &noted, 4), [at(0, 0), at(3, 2)]);
    }
}
