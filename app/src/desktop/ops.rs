//! Opérations sur l'état du bureau propres au processus du bureau (seul écrivain de
//! `desktop.json`) : onglets, ordre manuel, renommage, fusion de boîtes.
//!
//! Tout est pur (aucun fichier touché, sauf [`validate_new_name`] qui regarde si le nom
//! est libre) et testé.

use std::path::{Path, PathBuf};

use tidy_core::DesktopCommand;
use tidy_core::DesktopState;
use tidy_core::model::{BoxId, BoxKind, DeskBox, ItemRef, Tab};

/// Range `names` dans l'onglet `tab` de la boîte `b` (en les retirant de toute autre
/// boîte), par [`DesktopCommand::Assign`] puis un déplacement vers l'onglet voulu.
/// `before` : insérer avant cet élément (ordre manuel), sinon à la fin.
pub fn assign_to_tab(
    state: &mut DesktopState,
    names: &[String],
    b: BoxId,
    tab: usize,
    before: Option<&str>,
) {
    if names.is_empty() || state.find_box(b).is_none() {
        return;
    }
    let items: Vec<(ItemRef, Option<BoxId>)> = names
        .iter()
        .map(|n| (ItemRef(n.clone()), Some(b)))
        .collect();
    state.apply(DesktopCommand::Assign { items });
    let Some(deskbox) = state.find_box_mut(b) else {
        return;
    };
    // `Assign` range dans le premier onglet : déplacer vers l'onglet voulu.
    let tab = tab.min(deskbox.tabs.len().saturating_sub(1));
    if tab != 0 {
        deskbox.tabs[0].items.retain(|i| !names.contains(&i.0));
    }
    move_in_tab(&mut deskbox.tabs[tab], names, before);
}

/// Place `names` dans `tab`, avant `before` (ou à la fin), dans l'ordre donné. Les
/// éléments déjà présents sont déplacés, les autres ajoutés.
pub fn move_in_tab(tab: &mut Tab, names: &[String], before: Option<&str>) {
    tab.items.retain(|i| !names.contains(&i.0));
    let at = before
        .and_then(|name| tab.items.iter().position(|i| i.0 == name))
        .unwrap_or(tab.items.len());
    for (k, name) in names.iter().enumerate() {
        tab.items.insert(at + k, ItemRef(name.clone()));
    }
}

/// Retire des éléments de la boîte `b` : ils redeviennent des icônes du bureau.
pub fn remove_from_box(state: &mut DesktopState, names: &[String]) {
    let items = names.iter().map(|n| (ItemRef(n.clone()), None)).collect();
    state.apply(DesktopCommand::Assign { items });
}

/// Oublie des éléments qui n'existent plus (mis à la corbeille par Tidy).
pub fn forget_items(state: &mut DesktopState, names: &[String]) {
    for b in &mut state.boxes {
        for tab in &mut b.tabs {
            tab.items.retain(|i| !names.contains(&i.0));
        }
    }
    state.unassigned.retain(|i| !names.contains(&i.0));
    for name in names {
        state.loose.remove(&ItemRef(name.clone()));
    }
}

/// Oublie les icônes hors boîte dont le fichier n'existe plus. Rend `true` si l'état a
/// changé.
pub fn prune_loose(state: &mut DesktopState, exists: impl Fn(&str) -> bool) -> bool {
    let before = state.loose.len();
    state.loose.retain(|item, _| exists(&item.0));
    state.loose.len() != before
}

/// Un élément a été renommé sur le disque : il garde sa place (boîte, onglet, rang).
pub fn rename_item(state: &mut DesktopState, old: &str, new: &str) {
    let rename = |i: &mut ItemRef| {
        if i.0 == old {
            i.0 = new.to_owned();
        }
    };
    for b in &mut state.boxes {
        for tab in &mut b.tabs {
            tab.items.iter_mut().for_each(rename);
        }
    }
    state.unassigned.iter_mut().for_each(rename);
    if let Some(icon) = state.loose.remove(&ItemRef(old.to_owned())) {
        state.loose.insert(ItemRef(new.to_owned()), icon);
    }
}

/// Fusionne la boîte `from` dans `into` : ses onglets s'ajoutent à ceux de `into`, ses
/// règles la suivent, puis elle disparaît. Aucun élément ne revient au bureau.
pub fn merge_boxes(state: &mut DesktopState, into: BoxId, from: BoxId) -> bool {
    if into == from || state.find_box(into).is_none() {
        return false;
    }
    let Some(index) = state.boxes.iter().position(|b| b.id == from) else {
        return false;
    };
    if !matches!(state.boxes[index].kind, BoxKind::Normal) {
        return false;
    }
    let removed = state.boxes.remove(index);
    let Some(target) = state.find_box_mut(into) else {
        return false;
    };
    // Un onglet sans nom prend celui de sa boîte d'origine.
    let name_of = |tab: &Tab, b: &DeskBox| {
        if tab.name.is_empty() {
            b.name.clone()
        } else {
            tab.name.clone()
        }
    };
    if target.tabs.len() == 1 && target.tabs[0].name.is_empty() {
        target.tabs[0].name = target.name.clone();
    }
    for tab in &removed.tabs {
        let mut tab = tab.clone();
        tab.name = name_of(&tab, &removed);
        target.tabs.push(tab);
    }
    for rule in &mut state.rules {
        if rule.target == from {
            rule.target = into;
        }
    }
    true
}

/// Détache l'onglet `index` de la boîte `b` dans une nouvelle boîte (même apparence,
/// même tri). Rend la nouvelle boîte, à placer par l'appelant.
pub fn detach_tab(state: &mut DesktopState, b: BoxId, index: usize) -> Option<BoxId> {
    let source = state.find_box_mut(b)?;
    if source.tabs.len() < 2 || index >= source.tabs.len() {
        return None;
    }
    let mut tab = source.tabs.remove(index);
    if source.active_tab >= source.tabs.len() || source.active_tab > index {
        source.active_tab = source.active_tab.saturating_sub(1);
    }
    let name = if tab.name.is_empty() {
        source.name.clone()
    } else {
        std::mem::take(&mut tab.name)
    };
    let mut new = DeskBox::new(name);
    new.tabs = vec![tab];
    new.appearance = source.appearance.clone();
    new.sort = source.sort;
    new.view = source.view;
    let id = new.id;
    state.boxes.push(new);
    Some(id)
}

/// Pourquoi un nouveau nom est refusé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameError {
    Empty,
    /// Caractère interdit (séparateur de dossiers, ou interdit par Windows).
    Invalid,
    /// Un autre élément porte déjà ce nom.
    Exists,
}

/// Caractères refusés dans un nom, sur tous les systèmes (pour qu'un bureau synchronisé
/// reste lisible sous Windows).
const FORBIDDEN: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Vérifie que `old` peut s'appeler `new` dans `dir`. Rend le nouveau chemin.
/// Changer seulement la casse est permis (même fichier sur un disque insensible à la
/// casse).
pub fn validate_new_name(dir: &Path, old: &str, new: &str) -> Result<PathBuf, NameError> {
    let new = new.trim();
    if new.is_empty() || new == "." || new == ".." {
        return Err(NameError::Empty);
    }
    if new
        .chars()
        .any(|c| FORBIDDEN.contains(&c) || c.is_control())
    {
        return Err(NameError::Invalid);
    }
    let target = dir.join(new);
    let same_file = new.to_lowercase() == old.to_lowercase();
    if !same_file && std::fs::symlink_metadata(&target).is_ok() {
        return Err(NameError::Exists);
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidy_core::rules::Rule;

    fn names(tab: &Tab) -> Vec<&str> {
        tab.items.iter().map(|i| i.0.as_str()).collect()
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn move_in_tab_inserts_before_and_reorders() {
        let mut tab = Tab::new("");
        move_in_tab(&mut tab, &s(&["a", "b", "c"]), None);
        assert_eq!(names(&tab), ["a", "b", "c"]);
        move_in_tab(&mut tab, &s(&["c"]), Some("a"));
        assert_eq!(names(&tab), ["c", "a", "b"]);
        move_in_tab(&mut tab, &s(&["c", "a"]), None);
        assert_eq!(names(&tab), ["b", "c", "a"]);
        // `before` déplacé lui-même : à la fin.
        move_in_tab(&mut tab, &s(&["b"]), Some("b"));
        assert_eq!(names(&tab), ["c", "a", "b"]);
    }

    #[test]
    fn assign_goes_to_the_active_tab_and_leaves_other_boxes() {
        let mut state = DesktopState::default();
        let mut a = DeskBox::new("A");
        a.tabs.push(Tab::new("Deux"));
        let b = DeskBox::new("B");
        let (ida, idb) = (a.id, b.id);
        state.boxes.extend([a, b]);
        assign_to_tab(&mut state, &s(&["x.pdf"]), idb, 0, None);
        assert_eq!(state.box_of(&ItemRef("x.pdf".into())), Some(idb));
        assign_to_tab(&mut state, &s(&["x.pdf", "y.png"]), ida, 1, None);
        let a = state.find_box(ida).unwrap();
        assert!(a.tabs[0].items.is_empty());
        assert_eq!(names(&a.tabs[1]), ["x.pdf", "y.png"]);
        assert!(
            !state
                .find_box(idb)
                .unwrap()
                .contains(&ItemRef("x.pdf".into()))
        );
    }

    #[test]
    fn removing_and_forgetting() {
        let mut state = DesktopState::default();
        let b = DeskBox::new("A");
        let id = b.id;
        state.boxes.push(b);
        assign_to_tab(&mut state, &s(&["x", "y"]), id, 0, None);
        remove_from_box(&mut state, &s(&["x"]));
        assert!(state.loose.contains_key(&ItemRef("x".into())));
        assert!(!prune_loose(&mut state, |n| n == "x"));
        assert!(prune_loose(&mut state, |_| false));
        assert!(state.loose.is_empty());
        forget_items(&mut state, &s(&["x", "y"]));
        assert!(state.loose.is_empty());
        assert!(state.find_box(id).unwrap().tabs[0].items.is_empty());
    }

    #[test]
    fn renaming_keeps_the_place() {
        let mut state = DesktopState::default();
        let b = DeskBox::new("A");
        let id = b.id;
        state.boxes.push(b);
        assign_to_tab(&mut state, &s(&["a", "b", "c"]), id, 0, None);
        rename_item(&mut state, "b", "B2");
        assert_eq!(
            names(&state.find_box(id).unwrap().tabs[0]),
            ["a", "B2", "c"]
        );
    }

    #[test]
    fn merge_moves_tabs_and_rules() {
        let mut state = DesktopState::default();
        let a = DeskBox::new("Docs");
        let mut b = DeskBox::new("Images");
        b.tabs[0].items.push(ItemRef("p.png".into()));
        let (ida, idb) = (a.id, b.id);
        state.boxes.extend([a, b]);
        state
            .rules
            .push(Rule::category(idb, tidy_core::category::Category::Image));
        assert!(merge_boxes(&mut state, ida, idb));
        assert_eq!(state.boxes.len(), 1);
        let a = state.find_box(ida).unwrap();
        assert_eq!(a.tabs.len(), 2);
        assert_eq!(a.tabs[0].name, "Docs");
        assert_eq!(a.tabs[1].name, "Images");
        assert_eq!(state.rules[0].target, ida);
        // Rien n'est revenu au bureau.
        assert!(state.loose.is_empty());
        assert!(!merge_boxes(&mut state, ida, ida));
    }

    #[test]
    fn detach_creates_a_box_with_the_tab() {
        let mut state = DesktopState::default();
        let mut a = DeskBox::new("A");
        a.tabs[0].name = "Un".into();
        let mut two = Tab::new("Deux");
        two.items.push(ItemRef("z".into()));
        a.tabs.push(two);
        a.active_tab = 1;
        let ida = a.id;
        state.boxes.push(a);
        let new = detach_tab(&mut state, ida, 1).unwrap();
        let a = state.find_box(ida).unwrap();
        assert_eq!(a.tabs.len(), 1);
        assert_eq!(a.active_tab, 0);
        let n = state.find_box(new).unwrap();
        assert_eq!(n.name, "Deux");
        assert_eq!(names(&n.tabs[0]), ["z"]);
        // Le dernier onglet ne se détache pas.
        assert_eq!(detach_tab(&mut state, ida, 0), None);
    }

    #[test]
    fn new_names_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
        std::fs::write(dir.path().join("b.txt"), b"x").unwrap();
        let d = dir.path();
        assert_eq!(validate_new_name(d, "a.txt", "  "), Err(NameError::Empty));
        assert_eq!(
            validate_new_name(d, "a.txt", "x/y"),
            Err(NameError::Invalid)
        );
        assert_eq!(
            validate_new_name(d, "a.txt", "x:y"),
            Err(NameError::Invalid)
        );
        assert_eq!(
            validate_new_name(d, "a.txt", "b.txt"),
            Err(NameError::Exists)
        );
        assert_eq!(validate_new_name(d, "a.txt", "c.txt"), Ok(d.join("c.txt")));
        // Changer la casse est permis.
        assert_eq!(validate_new_name(d, "a.txt", "A.txt"), Ok(d.join("A.txt")));
    }
}
