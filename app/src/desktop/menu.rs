//! Menus de Tidy (menu ☰ d'une boîte, menu d'éléments quand le menu natif n'est pas
//! disponible).
//!
//! Une boîte est une petite fenêtre : un menu egui ordinaire y serait coupé. Les menus
//! s'affichent donc dans une fenêtre flottante à part (voir [`super::overlay`]), et les
//! sous-menus (« Trier par ▸ ») y remplacent le menu principal, avec une entrée
//! « Retour ». Le contenu est décrit par des données ([`Entry`]), testées.
//!
//! Le menu ☰ suit celui de l'original : « Créer une boîte ▸ », « Affichage ▸ »,
//! « Trier par ▸ », « Règles ▸ », puis les réglages de la boîte (apparence, réglages,
//! icônes, nom, gel) et sa suppression.

use egui::{Align2, CornerRadius, FontId, Sense, Ui, vec2};
use tidy_core::category::Category;
use tidy_core::model::{BoxId, BoxKind, DeskBox, SortKey, ViewMode};
use tidy_ui::{icons, t, theme};

use super::action::Action;
use super::targets::{self, Targets};

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
    /// « Créer une boîte ▸ ».
    Create,
    /// « Règles ▸ ».
    Rules,
    /// « Règles ▸ Ranger ici ▸ » : les catégories de la page Rangement.
    RulesTargets,
}

/// Ce que le menu ☰ doit savoir en plus de la boîte.
pub struct MenuCx<'a> {
    /// Les autres boîtes ordinaires (pour « Fusionner avec »).
    pub others: &'a [(BoxId, String)],
    pub icon_size: f32,
    /// Ce qui est rangé dans cette boîte (page Rangement).
    pub targets: &'a Targets,
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

/// Menu ☰ d'une boîte, à la page `page`.
pub fn box_menu(b: &DeskBox, page: Page, cx: &MenuCx) -> Vec<Entry> {
    let id = b.id;
    let normal = matches!(b.kind, BoxKind::Normal);
    let others = cx.others;
    match page {
        Page::Main => {
            let mut e = vec![
                item(
                    icons::PLUS,
                    t!("box_extra.menu_create"),
                    EntryKind::Open(Page::Create),
                ),
                Entry::Separator,
                item(
                    icons::GRID,
                    t!("box.menu_view"),
                    EntryKind::Open(Page::View),
                ),
                item(
                    icons::SORT,
                    t!("box.menu_sort"),
                    EntryKind::Open(Page::Sort),
                ),
            ];
            if normal {
                e.push(item(
                    icons::AUTO_TIDY,
                    t!("box_extra.menu_rules"),
                    EntryKind::Open(Page::Rules),
                ));
            }
            e.extend([
                Entry::Separator,
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
            ]);
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
                    icons::SETTINGS,
                    t!("box_extra.menu_settings"),
                    EntryKind::Do(Action::BoxSettings { b: id }),
                ),
                item(
                    icons::REFRESH,
                    t!("box_extra.menu_refresh_icons"),
                    EntryKind::Do(Action::RefreshIcons { b: id }),
                ),
                item(
                    icons::EDIT,
                    t!("box.menu_rename_box"),
                    EntryKind::Do(Action::StartRenameBox { b: id }),
                ),
                check(
                    icons::LOCK,
                    t!("box_extra.menu_freeze"),
                    b.frozen,
                    Action::ToggleFreeze { b: id },
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
        Page::Create => vec![
            back(),
            Entry::Separator,
            item(
                icons::BOX,
                t!("box_extra.create_box"),
                EntryKind::Do(Action::NewBox {
                    near: Some(id),
                    rect: None,
                }),
            ),
            item(
                icons::PORTAL,
                t!("box_extra.create_portal"),
                EntryKind::Do(Action::NewPortal { near: id }),
            ),
            // Lot F : « Boîte privée »
        ],
        Page::Rules => vec![
            back(),
            Entry::Separator,
            item(
                icons::INBOX,
                t!("box_extra.rules_apply"),
                EntryKind::Do(Action::ApplyRules),
            ),
            item(
                icons::FILTER,
                t!("box_extra.rules_targets"),
                EntryKind::Open(Page::RulesTargets),
            ),
            Entry::Separator,
            item(
                icons::SETTINGS,
                t!("box_extra.rules_open"),
                EntryKind::Do(Action::OpenRules),
            ),
        ],
        Page::RulesTargets => {
            let mut e = vec![
                item(
                    icons::BACK,
                    t!("box.menu_back"),
                    EntryKind::Open(Page::Rules),
                ),
                Entry::Separator,
            ];
            for category in Category::ALL {
                e.push(check(
                    targets::icon(category),
                    targets::label(category),
                    cx.targets.categories.contains(&category),
                    Action::ToggleRuleTarget {
                        b: id,
                        category: Some(category),
                    },
                ));
            }
            e.push(Entry::Separator);
            e.push(check(
                icons::INBOX,
                t!("box_extra.rules_catch_all"),
                cx.targets.catch_all,
                Action::ToggleRuleTarget {
                    b: id,
                    category: None,
                },
            ));
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
            // Grille libre : revenir à des éléments qui se suivent.
            if super::items::free_grid(b).is_some_and(|cells| !cells.is_empty()) {
                e.push(item(
                    icons::GRID,
                    t!("box.compact_grid"),
                    EntryKind::Do(Action::CompactGrid { b: id }),
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
                    (cx.icon_size - size).abs() < 0.5,
                    Action::SetIconSize { b: id, size },
                ));
            }
            e.extend([
                Entry::Separator,
                check(
                    icons::SEARCH,
                    t!("box_extra.view_search"),
                    b.search_in_title,
                    Action::ToggleSearchInTitle { b: id },
                ),
                check(
                    icons::EYE_OFF,
                    t!("box_extra.view_exclude_quick_hide"),
                    b.exclude_from_quick_hide,
                    Action::ToggleExcludeQuickHide { b: id },
                ),
                check(
                    icons::ZAP,
                    t!("box_extra.view_exclude_quick_access"),
                    b.exclude_from_quick_access,
                    Action::ToggleExcludeQuickAccess { b: id },
                ),
            ]);
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
    // Hauteurs exactes (voir `height`) : pas d'espace entre les entrées.
    ui.spacing_mut().item_spacing.y = 0.0;
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
                    // Sous-menu (pas une entrée « Retour »).
                    (EntryKind::Open(_), _) if *icon != icons::BACK => {
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

    fn menu(b: &DeskBox, page: Page, others: &[(BoxId, String)]) -> Vec<Entry> {
        menu_with(b, page, others, &Targets::default())
    }

    fn menu_with(
        b: &DeskBox,
        page: Page,
        others: &[(BoxId, String)],
        targets: &Targets,
    ) -> Vec<Entry> {
        box_menu(
            b,
            page,
            &MenuCx {
                others,
                icon_size: 48.0,
                targets,
            },
        )
    }

    /// Entrée cochée (`Some`) ou simple (`None`) qui déclenche `action`.
    fn checked_for(entries: &[Entry], action: &Action) -> Option<Option<bool>> {
        entries.iter().find_map(|e| match e {
            Entry::Item {
                kind: EntryKind::Do(a),
                checked,
                ..
            } if a == action => Some(*checked),
            _ => None,
        })
    }

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
        let main = menu(&b, Page::Main, &[]);
        let kinds = actions(&main);
        assert!(
            !kinds
                .iter()
                .any(|k| matches!(k, EntryKind::Do(Action::DetachTab { .. })))
        );
        assert!(!kinds.contains(&&EntryKind::Open(Page::Merge)));
        b.tabs.push(Tab::new("Deux"));
        let other = (BoxId::new(), "B".to_owned());
        let main = menu(&b, Page::Main, std::slice::from_ref(&other));
        let kinds = actions(&main);
        assert!(
            kinds
                .iter()
                .any(|k| matches!(k, EntryKind::Do(Action::DetachTab { .. })))
        );
        assert!(kinds.contains(&&EntryKind::Open(Page::Merge)));
        let merge = menu(&b, Page::Merge, std::slice::from_ref(&other));
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
        let sort = menu(&b, Page::Sort, &[]);
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

    #[test]
    fn view_toggles_follow_the_box() {
        let mut b = DeskBox::new("A");
        let id = b.id;
        let view = menu(&b, Page::View, &[]);
        for action in [
            Action::ToggleSearchInTitle { b: id },
            Action::ToggleExcludeQuickHide { b: id },
            Action::ToggleExcludeQuickAccess { b: id },
        ] {
            assert_eq!(checked_for(&view, &action), Some(Some(false)), "{action:?}");
        }
        b.search_in_title = true;
        b.exclude_from_quick_hide = true;
        b.exclude_from_quick_access = true;
        let view = menu(&b, Page::View, &[]);
        for action in [
            Action::ToggleSearchInTitle { b: id },
            Action::ToggleExcludeQuickHide { b: id },
            Action::ToggleExcludeQuickAccess { b: id },
        ] {
            assert_eq!(checked_for(&view, &action), Some(Some(true)), "{action:?}");
        }
    }

    #[test]
    fn freeze_is_a_check_in_the_main_menu() {
        let mut b = DeskBox::new("A");
        let freeze = Action::ToggleFreeze { b: b.id };
        assert_eq!(
            checked_for(&menu(&b, Page::Main, &[]), &freeze),
            Some(Some(false))
        );
        b.frozen = true;
        assert_eq!(
            checked_for(&menu(&b, Page::Main, &[]), &freeze),
            Some(Some(true))
        );
        // Réglages, actualiser les icônes : entrées simples.
        let main = menu(&b, Page::Main, &[]);
        assert_eq!(
            checked_for(&main, &Action::BoxSettings { b: b.id }),
            Some(None)
        );
        assert_eq!(
            checked_for(&main, &Action::RefreshIcons { b: b.id }),
            Some(None)
        );
    }

    #[test]
    fn submenus_create_and_rules() {
        let b = DeskBox::new("A");
        let main = menu(&b, Page::Main, &[]);
        let kinds = actions(&main);
        assert!(kinds.contains(&&EntryKind::Open(Page::Create)));
        assert!(kinds.contains(&&EntryKind::Open(Page::Rules)));
        let create = menu(&b, Page::Create, &[]);
        let kinds = actions(&create);
        assert!(kinds.contains(&&EntryKind::Do(Action::NewBox {
            near: Some(b.id),
            rect: None
        })));
        assert!(kinds.contains(&&EntryKind::Do(Action::NewPortal { near: b.id })));
        let rules = menu(&b, Page::Rules, &[]);
        let kinds = actions(&rules);
        assert!(kinds.contains(&&EntryKind::Do(Action::ApplyRules)));
        assert!(kinds.contains(&&EntryKind::Do(Action::OpenRules)));
        assert!(kinds.contains(&&EntryKind::Open(Page::RulesTargets)));
        // Un portail ne reçoit pas de règles.
        let mut portal = DeskBox::new("P");
        portal.kind = BoxKind::Portal {
            path: std::path::PathBuf::from("/tmp"),
        };
        let kinds = actions(&menu(&portal, Page::Main, &[]))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        assert!(!kinds.contains(&EntryKind::Open(Page::Rules)));
    }

    #[test]
    fn rule_targets_are_checked() {
        let b = DeskBox::new("A");
        let targets = Targets {
            categories: vec![Category::Image],
            catch_all: true,
        };
        let page = menu_with(&b, Page::RulesTargets, &[], &targets);
        let image = Action::ToggleRuleTarget {
            b: b.id,
            category: Some(Category::Image),
        };
        let music = Action::ToggleRuleTarget {
            b: b.id,
            category: Some(Category::Music),
        };
        let rest = Action::ToggleRuleTarget {
            b: b.id,
            category: None,
        };
        assert_eq!(checked_for(&page, &image), Some(Some(true)));
        assert_eq!(checked_for(&page, &music), Some(Some(false)));
        assert_eq!(checked_for(&page, &rest), Some(Some(true)));
        // Une entrée par catégorie, plus le fourre-tout.
        let checks = page
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Entry::Item {
                        checked: Some(_),
                        ..
                    }
                )
            })
            .count();
        assert_eq!(checks, Category::ALL.len() + 1);
        // « Retour » ramène au sous-menu Règles.
        assert!(actions(&page).contains(&&EntryKind::Open(Page::Rules)));
    }
}
