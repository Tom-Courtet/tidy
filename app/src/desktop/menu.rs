//! Menus de Tidy (menu ☰ d'une boîte, menu d'éléments quand le menu natif n'est pas
//! disponible).
//!
//! Une boîte est une petite fenêtre : un menu egui ordinaire y serait coupé. Les menus
//! s'affichent donc dans une fenêtre flottante à part (voir [`super::overlay`]), et les
//! sous-menus (« Trier par ▸ ») y remplacent le menu principal, avec une entrée
//! « Retour ». Le contenu est décrit par des données ([`Entry`]), testées.

use egui::{Align2, CornerRadius, FontId, Sense, Ui, vec2};
use tidy_core::model::{BoxId, BoxKind, DeskBox, SortKey, ViewMode};
use tidy_ui::{icons, t, theme};

use super::action::Action;

/// Largeur d'un menu.
pub const WIDTH: f32 = 252.0;
const ROW_H: f32 = 30.0;
const SEPARATOR_H: f32 = 9.0;
/// Marge intérieure du cadre.
pub const PADDING: f32 = 6.0;

/// Tailles d'icônes proposées.
pub const ICON_SIZES: [(f32, &str); 3] = [(32.0, "small"), (48.0, "medium"), (64.0, "large")];

/// Page d'un menu de boîte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Main,
    Sort,
    View,
    Merge,
}

/// Ce que fait une entrée.
#[derive(Debug, Clone, PartialEq)]
pub enum EntryKind {
    Do(Action),
    Open(Page),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Item {
        icon: &'static str,
        label: String,
        kind: EntryKind,
        /// Case cochée (`Some`) ou entrée simple.
        checked: Option<bool>,
        danger: bool,
    },
    Separator,
}

fn item(icon: &'static str, label: impl Into<String>, kind: EntryKind) -> Entry {
    Entry::Item {
        icon,
        label: label.into(),
        kind,
        checked: None,
        danger: false,
    }
}

fn check(icon: &'static str, label: impl Into<String>, on: bool, action: Action) -> Entry {
    Entry::Item {
        icon,
        label: label.into(),
        kind: EntryKind::Do(action),
        checked: Some(on),
        danger: false,
    }
}

fn back() -> Entry {
    item(
        icons::BACK,
        t!("box.menu_back"),
        EntryKind::Open(Page::Main),
    )
}

/// Libellé de « Afficher dans l'Explorateur / le Finder ».
pub fn reveal_label() -> &'static str {
    if cfg!(windows) {
        t!("box.menu_reveal_explorer")
    } else {
        t!("box.menu_reveal_finder")
    }
}

/// Menu d'éléments (repli du menu natif). `portal` : éléments d'un portail (ni
/// « Retirer de la boîte »).
pub fn item_menu(b: BoxId, names: &[String], portal: bool) -> Vec<Entry> {
    let names = names.to_vec();
    let mut entries = vec![item(
        icons::OPEN,
        t!("box.menu_open"),
        EntryKind::Do(Action::Open {
            b,
            names: names.clone(),
        }),
    )];
    if let [name] = names.as_slice() {
        entries.push(item(
            icons::FOLDER_OPEN,
            reveal_label(),
            EntryKind::Do(Action::Reveal {
                b,
                name: name.clone(),
            }),
        ));
        entries.push(item(
            icons::EDIT,
            t!("box.menu_rename"),
            EntryKind::Do(Action::StartRename {
                b,
                name: name.clone(),
            }),
        ));
    }
    entries.push(item(
        icons::COPY,
        t!("box.menu_copy_path"),
        EntryKind::Do(Action::CopyPath {
            b,
            names: names.clone(),
        }),
    ));
    entries.push(Entry::Separator);
    if !portal {
        entries.push(item(
            icons::RESTORE,
            t!("box.menu_remove_from_box"),
            EntryKind::Do(Action::RemoveFromBox {
                b,
                names: names.clone(),
            }),
        ));
    }
    entries.push(Entry::Item {
        icon: icons::DELETE,
        label: t!("box.menu_trash").into(),
        kind: EntryKind::Do(Action::AskTrash { b, names }),
        checked: None,
        danger: true,
    });
    entries
}

/// Menu ☰ d'une boîte, à la page `page`. `others` : les autres boîtes ordinaires
/// (pour « Fusionner avec »).
pub fn box_menu(b: &DeskBox, page: Page, others: &[(BoxId, String)], icon_size: f32) -> Vec<Entry> {
    let id = b.id;
    let normal = matches!(b.kind, BoxKind::Normal);
    match page {
        Page::Main => {
            let mut e = vec![
                item(
                    icons::EDIT,
                    t!("box.menu_rename_box"),
                    EntryKind::Do(Action::StartRenameBox { b: id }),
                ),
                item(
                    icons::SORT,
                    t!("box.menu_sort"),
                    EntryKind::Open(Page::Sort),
                ),
                item(
                    icons::GRID,
                    t!("box.menu_view"),
                    EntryKind::Open(Page::View),
                ),
                if b.rolled_up {
                    item(
                        icons::ROLL_DOWN,
                        t!("box.menu_unroll"),
                        EntryKind::Do(Action::ToggleRoll { b: id }),
                    )
                } else {
                    item(
                        icons::ROLL_UP,
                        t!("box.menu_roll"),
                        EntryKind::Do(Action::ToggleRoll { b: id }),
                    )
                },
                check(
                    icons::ROLL_UP,
                    t!("box.menu_auto_roll"),
                    b.auto_roll,
                    Action::ToggleAutoRoll { b: id },
                ),
                Entry::Separator,
            ];
            if normal {
                e.push(item(
                    icons::TABS,
                    t!("box.menu_new_tab"),
                    EntryKind::Do(Action::NewTab { b: id }),
                ));
                if !others.is_empty() {
                    e.push(item(
                        icons::TABS,
                        t!("box.menu_merge"),
                        EntryKind::Open(Page::Merge),
                    ));
                }
                if b.tabs.len() > 1 {
                    e.push(item(
                        icons::TABS,
                        t!("box.menu_detach_tab"),
                        EntryKind::Do(Action::DetachTab {
                            b: id,
                            index: b.active_tab.min(b.tabs.len() - 1),
                        }),
                    ));
                }
                e.push(Entry::Separator);
            }
            e.extend([
                item(
                    icons::PALETTE,
                    t!("box.menu_appearance"),
                    EntryKind::Do(Action::Appearance { b: id }),
                ),
                item(
                    icons::PORTAL,
                    t!("box.menu_new_portal"),
                    EntryKind::Do(Action::NewPortal { near: id }),
                ),
                item(
                    icons::PLUS,
                    t!("box.menu_new_box"),
                    EntryKind::Do(Action::NewBox {
                        near: Some(id),
                        rect: None,
                    }),
                ),
                Entry::Separator,
                Entry::Item {
                    icon: icons::DELETE,
                    label: t!("box.menu_delete_box").into(),
                    kind: EntryKind::Do(Action::AskDeleteBox { b: id }),
                    checked: None,
                    danger: true,
                },
            ]);
            e
        }
        Page::Sort => {
            let mut e = vec![back(), Entry::Separator];
            for (key, label) in [
                (SortKey::Manual, t!("box.sort_manual")),
                (SortKey::Name, t!("box.sort_name")),
                (SortKey::Kind, t!("box.sort_kind")),
                (SortKey::Modified, t!("box.sort_modified")),
                (SortKey::Size, t!("box.sort_size")),
            ] {
                e.push(check(
                    icons::SORT,
                    label,
                    b.sort.key == key,
                    Action::SetSort { b: id, key },
                ));
            }
            e.push(Entry::Separator);
            e.push(check(
                icons::CHEVRON_DOWN,
                t!("box.sort_descending"),
                b.sort.descending,
                Action::ToggleDescending { b: id },
            ));
            e.push(check(
                icons::CALENDAR,
                t!("box.sort_group_by_date"),
                b.sort.group_by_date,
                Action::ToggleGroupByDate { b: id },
            ));
            e
        }
        Page::View => {
            let mut e = vec![
                back(),
                Entry::Separator,
                check(
                    icons::GRID,
                    t!("box.view_grid"),
                    b.view == ViewMode::Grid,
                    Action::SetView {
                        b: id,
                        view: ViewMode::Grid,
                    },
                ),
                check(
                    icons::LIST,
                    t!("box.view_list"),
                    b.view == ViewMode::List,
                    Action::SetView {
                        b: id,
                        view: ViewMode::List,
                    },
                ),
                Entry::Separator,
            ];
            for (size, key) in ICON_SIZES {
                let label = match key {
                    "small" => t!("box.icons_small"),
                    "medium" => t!("box.icons_medium"),
                    _ => t!("box.icons_large"),
                };
                e.push(check(
                    icons::IMAGE,
                    label,
                    (icon_size - size).abs() < 0.5,
                    Action::SetIconSize { b: id, size },
                ));
            }
            e
        }
        Page::Merge => {
            let mut e = vec![back(), Entry::Separator];
            for (other, name) in others {
                e.push(item(
                    icons::BOX,
                    name.clone(),
                    EntryKind::Do(Action::MergeWith {
                        b: id,
                        other: *other,
                    }),
                ));
            }
            e
        }
    }
}

/// Hauteur du menu, cadre compris.
pub fn height(entries: &[Entry]) -> f32 {
    2.0 * PADDING
        + entries
            .iter()
            .map(|e| match e {
                Entry::Item { .. } => ROW_H,
                Entry::Separator => SEPARATOR_H,
            })
            .sum::<f32>()
}

/// Dessine les entrées ; rend celle qui est cliquée.
pub fn draw(ui: &mut Ui, entries: &[Entry]) -> Option<EntryKind> {
    let mut chosen = None;
    let width = ui.available_width();
    for entry in entries {
        match entry {
            Entry::Separator => {
                let (rect, _) = ui.allocate_exact_size(vec2(width, SEPARATOR_H), Sense::hover());
                ui.painter().hline(
                    rect.x_range().shrink(6.0),
                    rect.center().y,
                    egui::Stroke::new(1.0, theme::BORDER),
                );
            }
            Entry::Item {
                icon,
                label,
                kind,
                checked,
                danger,
            } => {
                let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                let painter = ui.painter();
                if resp.hovered() {
                    let fill = if *danger {
                        theme::DANGER.gamma_multiply(0.18)
                    } else {
                        theme::SURFACE_HI
                    };
                    painter.rect_filled(rect, CornerRadius::same(8), fill);
                }
                let color = if *danger { theme::DANGER } else { theme::TEXT };
                painter.text(
                    rect.left_center() + vec2(10.0, 0.0),
                    Align2::LEFT_CENTER,
                    *icon,
                    FontId::proportional(theme::SIZE_M),
                    if *danger {
                        theme::DANGER
                    } else {
                        theme::TEXT_DIM
                    },
                );
                painter.text(
                    rect.left_center() + vec2(34.0, 0.0),
                    Align2::LEFT_CENTER,
                    tidy_ui::widgets::truncate(label, 30),
                    FontId::proportional(theme::SIZE_M),
                    color,
                );
                let right = rect.right_center() - vec2(10.0, 0.0);
                match (kind, checked) {
                    (_, Some(true)) => {
                        painter.text(
                            right,
                            Align2::RIGHT_CENTER,
                            icons::OK,
                            FontId::proportional(theme::SIZE_M),
                            theme::ACCENT_2,
                        );
                    }
                    (EntryKind::Open(page), _) if *page != Page::Main => {
                        painter.text(
                            right,
                            Align2::RIGHT_CENTER,
                            icons::CHEVRON_RIGHT,
                            FontId::proportional(theme::SIZE_M),
                            theme::TEXT_DIM,
                        );
                    }
                    _ => {}
                }
                if resp.clicked() {
                    chosen = Some(kind.clone());
                }
            }
        }
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidy_core::model::Tab;

    fn actions(entries: &[Entry]) -> Vec<&EntryKind> {
        entries
            .iter()
            .filter_map(|e| match e {
                Entry::Item { kind, .. } => Some(kind),
                Entry::Separator => None,
            })
            .collect()
    }

    #[test]
    fn tab_entries_depend_on_the_box() {
        let mut b = DeskBox::new("A");
        let main = box_menu(&b, Page::Main, &[], 48.0);
        let kinds = actions(&main);
        assert!(
            !kinds
                .iter()
                .any(|k| matches!(k, EntryKind::Do(Action::DetachTab { .. })))
        );
        assert!(!kinds.contains(&&EntryKind::Open(Page::Merge)));
        b.tabs.push(Tab::new("Deux"));
        let other = (BoxId::new(), "B".to_owned());
        let main = box_menu(&b, Page::Main, std::slice::from_ref(&other), 48.0);
        let kinds = actions(&main);
        assert!(
            kinds
                .iter()
                .any(|k| matches!(k, EntryKind::Do(Action::DetachTab { .. })))
        );
        assert!(kinds.contains(&&EntryKind::Open(Page::Merge)));
        let merge = box_menu(&b, Page::Merge, std::slice::from_ref(&other), 48.0);
        assert!(actions(&merge).contains(&&EntryKind::Do(Action::MergeWith {
            b: b.id,
            other: other.0
        })));
    }

    #[test]
    fn portal_items_cannot_leave_a_box() {
        let b = BoxId::new();
        let one = ["a.txt".to_owned()];
        let menu = item_menu(b, &one, true);
        assert!(
            !actions(&menu)
                .iter()
                .any(|k| matches!(k, EntryKind::Do(Action::RemoveFromBox { .. })))
        );
        let menu = item_menu(b, &one, false);
        assert!(
            actions(&menu)
                .iter()
                .any(|k| matches!(k, EntryKind::Do(Action::RemoveFromBox { .. })))
        );
        // Plusieurs éléments : ni renommer ni afficher.
        let two = ["a".to_owned(), "b".to_owned()];
        let menu = item_menu(b, &two, false);
        assert!(
            !actions(&menu)
                .iter()
                .any(|k| matches!(k, EntryKind::Do(Action::StartRename { .. })))
        );
    }

    #[test]
    fn current_choices_are_checked() {
        let mut b = DeskBox::new("A");
        b.sort.key = SortKey::Size;
        let sort = box_menu(&b, Page::Sort, &[], 48.0);
        let checked: Vec<_> = sort
            .iter()
            .filter_map(|e| match e {
                Entry::Item {
                    checked: Some(true),
                    kind,
                    ..
                } => Some(kind.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            checked,
            [EntryKind::Do(Action::SetSort {
                b: b.id,
                key: SortKey::Size
            })]
        );
        assert!(height(&sort) > 2.0 * PADDING);
    }
}
