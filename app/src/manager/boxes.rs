//! Page « Boîtes » : activer les boîtes, les lister et les régler (nom, apparence,
//! présentation, exclusions, raccourci), en ajouter (boîte ou portail de dossier), en
//! supprimer (aucun fichier supprimé), et l'apparence par défaut.

use std::path::Path;

use egui::{RichText, TextEdit};
use tidy_core::model::{BoxKind, ViewMode};
use tidy_core::settings::DesktopMode;
use tidy_core::{BoxId, DeskBox, DesktopCommand};
use tidy_ui::theme;
use tidy_ui::{icons, t, widgets};

use super::appearance;
use super::pages::{card_title, page_header};
use super::shortcuts::{self, Slot};
use super::{Confirm, Manager};

/// Nom affiché d'une boîte (« Sans nom » si vide).
pub fn display_name(name: &str) -> String {
    if name.trim().is_empty() {
        t!("boxes_page.unnamed").to_owned()
    } else {
        name.to_owned()
    }
}

/// Boîte désignée par l'argument de page `boxes:<id>` (menu ☰ > Réglages… d'une
/// boîte), s'il est de cette forme.
pub fn box_from_arg(arg: &str) -> Option<BoxId> {
    let id = arg.strip_prefix("boxes:")?;
    serde_json::from_value(serde_json::Value::String(id.to_owned())).ok()
}

/// `path` est-il le dossier du bureau ou un dossier qu'il contient ? Un portail ne peut
/// pas montrer le bureau lui-même.
pub fn is_desktop_itself(path: &Path, desktop: &Path) -> bool {
    let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    canonical(path) == canonical(desktop)
}

pub fn show(m: &mut Manager, ui: &mut egui::Ui) {
    page_header(ui, icons::BOX, t!("manager.boxes"), t!("boxes_page.intro"));

    // Interrupteur général et ajout.
    widgets::card(ui, |ui| {
        let mut enabled = m.settings.desktop_mode != DesktopMode::Off;
        if widgets::setting_row(
            ui,
            icons::POWER,
            t!("boxes_page.enabled"),
            Some(t!("boxes_page.enabled_hint")),
            |ui| widgets::toggle(ui, &mut enabled).changed(),
        ) {
            m.settings.desktop_mode = if enabled {
                DesktopMode::Overlay
            } else {
                DesktopMode::Off
            };
            m.save();
        }
        widgets::row_separator(ui);
        ui.horizontal_wrapped(|ui| {
            if widgets::primary(ui, format!("{}  {}", icons::PLUS, t!("boxes_page.add"))).clicked()
            {
                add_box(m);
            }
            if ui
                .button(format!(
                    "{}  {}",
                    icons::PORTAL,
                    t!("boxes_page.add_portal")
                ))
                .on_hover_text(t!("boxes_page.add_portal_hint"))
                .clicked()
            {
                add_portal(m);
            }
        });
    });
    ui.add_space(20.0);

    widgets::section_title(
        ui,
        t!("boxes_page.list_title", count = m.desktop.boxes.len()),
    );
    ui.add_space(8.0);
    if m.desktop.boxes.is_empty() {
        widgets::card(ui, |ui| {
            widgets::hint(ui, t!("boxes_page.empty"));
        });
    }
    let ids: Vec<BoxId> = m.desktop.boxes.iter().map(|b| b.id).collect();
    for id in ids {
        box_card(m, ui, id);
        ui.add_space(10.0);
    }
    ui.add_space(10.0);

    // Apparence par défaut.
    widgets::card(ui, |ui| {
        card_title(ui, icons::PALETTE, t!("boxes_page.default_appearance"));
        widgets::hint(ui, t!("boxes_page.default_appearance_hint"));
        ui.add_space(10.0);
        let changed = appearance::editor(
            ui,
            &mut m.settings.default_appearance,
            t!("boxes_page.preview_title"),
        );
        if changed.any() {
            m.save_later();
        }
    });

    super::vault::show(m, ui);
}

fn add_box(m: &mut Manager) {
    let b = DeskBox::new(t!("desktop.new_box"));
    let id = b.id;
    if m.send(DesktopCommand::UpsertBox { b }) {
        m.drafts.open_box = Some(id);
        m.show_notice(t!("boxes_page.added"));
    }
}

fn add_portal(m: &mut Manager) {
    let Some(path) = rfd::FileDialog::new()
        .set_title(t!("boxes_page.portal_dialog"))
        .pick_folder()
    else {
        return;
    };
    if tidy_core::store::desktop_dirs()
        .iter()
        .any(|desktop| is_desktop_itself(&path, desktop))
    {
        m.error = Some(t!("boxes_page.portal_on_desktop").to_owned());
        return;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let mut b = DeskBox::new(name);
    b.kind = BoxKind::Portal { path };
    let id = b.id;
    if m.send(DesktopCommand::UpsertBox { b }) {
        m.drafts.open_box = Some(id);
        m.show_notice(t!("boxes_page.added"));
    }
}

/// Résumé d'une boîte : éléments, présentation, raccourci.
fn summary(b: &DeskBox) -> String {
    let mut parts = Vec::new();
    match &b.kind {
        BoxKind::Portal { path } => parts.push(path.display().to_string()),
        BoxKind::Private => parts.push(t!("boxes_page.private").to_owned()),
        BoxKind::Normal => parts.push(t!("boxes_page.items", count = b.items().count())),
    }
    parts.push(
        match b.view {
            ViewMode::Grid => t!("boxes_page.view_grid"),
            ViewMode::List => t!("boxes_page.view_list"),
        }
        .to_owned(),
    );
    if let Some(h) = &b.hotkey {
        parts.push(tidy_ui::hotkey::label(h));
    }
    parts.join(" · ")
}

fn box_card(m: &mut Manager, ui: &mut egui::Ui, id: BoxId) {
    let Some(b) = m.desktop.find_box(id).cloned() else {
        return;
    };
    let open = m.drafts.open_box == Some(id);
    let effective = b
        .appearance
        .clone()
        .unwrap_or_else(|| m.settings.default_appearance.clone());
    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.set_min_height(30.0);
            // Pastille aux couleurs de la boîte : fond et barre de titre.
            let (dot, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(dot, 5, appearance::color(effective.background));
            painter.rect_filled(
                egui::Rect::from_min_size(dot.min, egui::vec2(20.0, 7.0)),
                egui::CornerRadius {
                    nw: 5,
                    ne: 5,
                    sw: 0,
                    se: 0,
                },
                appearance::color(effective.title_background),
            );
            painter.rect_stroke(
                dot,
                5,
                egui::Stroke::new(1.0, theme::TEXT_DIM.gamma_multiply(0.6)),
                egui::StrokeKind::Outside,
            );
            ui.add_space(4.0);
            if matches!(b.kind, BoxKind::Portal { .. }) {
                ui.label(RichText::new(icons::PORTAL).color(theme::ACCENT_2));
            }
            name_field(m, ui, &b);
            if b.hidden {
                ui.label(widgets::dim(format!(
                    "{}  {}",
                    icons::EYE_OFF,
                    t!("system_items.hidden_badge")
                )))
                .on_hover_text(t!("system_items.hidden_box_hint"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let chevron = if open {
                    icons::CHEVRON_UP
                } else {
                    icons::CHEVRON_DOWN
                };
                let tip = if open {
                    t!("boxes_page.collapse")
                } else {
                    t!("boxes_page.expand")
                };
                if widgets::icon_button(ui, chevron, tip).clicked() {
                    m.drafts.open_box = if open { None } else { Some(id) };
                }
                if widgets::icon_button(ui, icons::DELETE, t!("boxes_page.delete")).clicked() {
                    m.confirm = Some(Confirm::DeleteBox(id));
                }
            });
        });
        ui.add(egui::Label::new(widgets::dim(summary(&b))).truncate());
        if open {
            ui.add_space(8.0);
            details(m, ui, &b);
        }
    });
}

/// Nom modifiable : enregistré quand on quitte le champ (ou Entrée).
fn name_field(m: &mut Manager, ui: &mut egui::Ui, b: &DeskBox) {
    let id = b.id;
    let draft = m
        .drafts
        .box_names
        .entry(id)
        .or_insert_with(|| b.name.clone());
    let response = ui.add(
        TextEdit::singleline(draft)
            .hint_text(t!("boxes_page.unnamed"))
            .font(theme::strong(theme::SIZE_L))
            .desired_width(260.0)
            .frame(egui::Frame::NONE),
    );
    response.clone().on_hover_text(t!("boxes_page.rename_hint"));
    if response.lost_focus() {
        let name = draft.trim().to_owned();
        if name != b.name {
            m.edit_box(id, |b| b.name = name);
        }
    } else if !response.has_focus() && *draft != b.name {
        // Renommée ailleurs (sur le bureau) : on suit.
        draft.clone_from(&b.name);
    }
}

fn details(m: &mut Manager, ui: &mut egui::Ui, b: &DeskBox) {
    let id = b.id;
    let mut edited = b.clone();

    if let BoxKind::Portal { path } = &b.kind {
        widgets::setting_row(
            ui,
            icons::FOLDER_TREE,
            t!("boxes_page.portal_folder"),
            Some(&path.display().to_string()),
            |ui| {
                if ui.button(t!("common.open")).clicked()
                    && let Err(e) = tidy_platform::current().open(path)
                {
                    m.error = Some(format!("{e:#}"));
                }
            },
        );
        widgets::row_separator(ui);
    }

    // Apparence propre, ou celle par défaut.
    let mut own = edited.appearance.is_some();
    if widgets::setting_row(
        ui,
        icons::PALETTE,
        t!("boxes_page.own_appearance"),
        Some(t!("boxes_page.own_appearance_hint")),
        |ui| widgets::toggle(ui, &mut own).changed(),
    ) {
        edited.appearance = own.then(|| m.settings.default_appearance.clone());
    }
    if let Some(a) = &mut edited.appearance {
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(theme::BG)
            .corner_radius(12)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                appearance::editor(ui, a, &display_name(&b.name));
            });
    }
    widgets::row_separator(ui);

    widgets::setting_row(ui, icons::LAYOUT_LIST, t!("boxes_page.view"), None, |ui| {
        widgets::choice_value(
            ui,
            &mut edited.view,
            ViewMode::List,
            t!("boxes_page.view_list"),
        );
        widgets::choice_value(
            ui,
            &mut edited.view,
            ViewMode::Grid,
            t!("boxes_page.view_grid"),
        );
    });
    widgets::row_separator(ui);
    widgets::setting_row(
        ui,
        icons::EYE,
        t!("boxes_page.exclude_quick_hide"),
        Some(t!("boxes_page.exclude_quick_hide_hint")),
        |ui| widgets::toggle(ui, &mut edited.exclude_from_quick_hide),
    );
    widgets::row_separator(ui);
    widgets::setting_row(
        ui,
        icons::BOXES,
        t!("boxes_page.exclude_quick_access"),
        Some(t!("boxes_page.exclude_quick_access_hint")),
        |ui| widgets::toggle(ui, &mut edited.exclude_from_quick_access),
    );
    widgets::row_separator(ui);
    // Lot M : recherche dans le titre, gel.
    widgets::setting_row(
        ui,
        icons::SEARCH,
        t!("box_extra.view_search"),
        Some(t!("box_extra.search_page_hint")),
        |ui| widgets::toggle(ui, &mut edited.search_in_title),
    );
    widgets::row_separator(ui);
    widgets::setting_row(
        ui,
        icons::LOCK,
        t!("box_extra.menu_freeze"),
        Some(t!("box_extra.frozen_tooltip")),
        |ui| widgets::toggle(ui, &mut edited.frozen),
    );
    widgets::row_separator(ui);
    widgets::setting_row(
        ui,
        icons::EYE_OFF,
        t!("system_items.hidden_box"),
        Some(t!("system_items.hidden_box_hint")),
        |ui| widgets::toggle(ui, &mut edited.hidden),
    );
    widgets::row_separator(ui);

    if edited != *b {
        // Le nom suit sa propre saisie (peut-être validée dans cette même image).
        m.edit_box(id, |current| {
            let name = std::mem::take(&mut current.name);
            *current = edited;
            current.name = name;
        });
    }
    shortcuts::row(m, ui, Slot::Box(id));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_of_one_box() {
        let b = BoxId::new();
        assert_eq!(box_from_arg(&format!("boxes:{}", b.0)), Some(b));
        assert_eq!(box_from_arg("boxes"), None);
        assert_eq!(box_from_arg("boxes:pas-un-identifiant"), None);
        assert_eq!(
            super::super::Page::from_arg(&format!("boxes:{}", b.0)),
            Some(super::super::Page::Boxes)
        );
    }

    #[test]
    fn only_the_desktop_itself_cannot_be_a_portal() {
        let dir = tempfile::tempdir().unwrap();
        let desktop = dir.path().join("Bureau");
        let inside = desktop.join("Projets");
        let outside = dir.path().join("Documents");
        for d in [&inside, &outside] {
            std::fs::create_dir_all(d).unwrap();
        }
        assert!(is_desktop_itself(&desktop, &desktop));
        // Même désigné par un détour.
        assert!(is_desktop_itself(&inside.join(".."), &desktop));
        // Un dossier du bureau, ou d'ailleurs, peut devenir un portail.
        assert!(!is_desktop_itself(&inside, &desktop));
        assert!(!is_desktop_itself(&outside, &desktop));
    }

    #[test]
    fn empty_names_are_shown_as_unnamed() {
        assert_eq!(display_name("Documents"), "Documents");
        assert!(!display_name("  ").trim().is_empty());
    }
}
