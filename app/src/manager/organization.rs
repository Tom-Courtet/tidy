//! Page « Rangement » : pour chaque type d'élément, la boîte cible, les extensions
//! (modifiables, avec « Réinitialiser ») et les alias ; les options du rangement
//! automatique ; « Appliquer aux non affectées », « Appliquer à toutes » et « Tout
//! annuler ». Tout est dans `settings.organization` ; le rangement lui-même est fait
//! par l'agent (`Request::ApplyRules`, `Request::UndoOrganize`).

use tidy_core::BoxId;
use tidy_core::category::Category;
use tidy_core::settings::CategorySetting;
use tidy_ui::theme;
use tidy_ui::{icons, t, widgets};

use super::pages::{card_title, page_header};
use super::{Confirm, Manager};
use crate::ipc::Request;

/// Icône d'une catégorie.
pub fn category_icon(category: Category) -> &'static str {
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
pub fn category_label(category: Category) -> &'static str {
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

/// Extensions tapées par l'utilisateur (« .JPG, png  webp ») : en minuscules, sans
/// point, sans doublon. Les saisies invalides sont ignorées.
pub fn parse_extensions(input: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in input.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let ext = part.trim().trim_start_matches('.').to_ascii_lowercase();
        let valid = !ext.is_empty()
            && ext.len() <= 16
            && ext
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if valid && !out.contains(&ext) {
            out.push(ext);
        }
    }
    out
}

/// Extensions effectives d'une catégorie : celles de l'utilisateur, sinon par défaut.
pub fn extensions_of(setting: Option<&CategorySetting>, category: Category) -> Vec<String> {
    setting
        .and_then(|s| s.extensions.clone())
        .unwrap_or_else(|| defaults(category))
}

fn defaults(category: Category) -> Vec<String> {
    category
        .default_extensions()
        .iter()
        .map(|e| (*e).to_owned())
        .collect()
}

/// Modifie le réglage d'une catégorie ; un réglage revenu aux valeurs par défaut est
/// retiré (le fichier de réglages reste court).
fn edit_category(m: &mut Manager, category: Category, edit: impl FnOnce(&mut CategorySetting)) {
    let categories = &mut m.settings.organization.categories;
    let setting = categories
        .entry(category)
        .or_insert_with(|| CategorySetting {
            target: None,
            extensions: None,
            include_aliases: false,
        });
    edit(setting);
    if setting
        .extensions
        .as_ref()
        .is_some_and(|e| *e == defaults(category))
    {
        setting.extensions = None;
    }
    if setting.target.is_none() && setting.extensions.is_none() && !setting.include_aliases {
        categories.remove(&category);
    }
    m.save();
}

pub fn show(m: &mut Manager, ui: &mut egui::Ui) {
    page_header(
        ui,
        icons::AUTO_TIDY,
        t!("manager.organization"),
        t!("organization_page.intro"),
    );

    // Ranger maintenant.
    widgets::card(ui, |ui| {
        card_title(ui, icons::ZAP, t!("organization_page.apply_title"));
        widgets::hint(ui, t!("organization_page.apply_hint"));
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if widgets::primary(
                ui,
                format!(
                    "{}  {}",
                    icons::INBOX,
                    t!("organization_page.apply_unassigned")
                ),
            )
            .clicked()
                && m.request(Request::ApplyRules { all: false })
            {
                m.show_notice(t!("organization_page.applied"));
            }
            if ui
                .button(format!(
                    "{}  {}",
                    icons::AUTO_TIDY,
                    t!("organization_page.apply_all")
                ))
                .clicked()
            {
                m.confirm = Some(Confirm::ApplyToAll);
            }
            if ui
                .button(format!("{}  {}", icons::UNDO, t!("organization_page.undo")))
                .clicked()
            {
                m.confirm = Some(Confirm::UndoOrganize);
            }
        });
    });
    ui.add_space(12.0);

    // Options du rangement automatique.
    let boxes: Vec<(BoxId, String)> = m
        .desktop
        .boxes
        .iter()
        .map(|b| (b.id, super::boxes::display_name(&b.name)))
        .collect();
    widgets::card(ui, |ui| {
        card_title(ui, icons::SETTINGS, t!("organization_page.options"));
        let o = &mut m.settings.organization;
        let mut changed = false;
        changed |= widgets::setting_row(
            ui,
            icons::BELL,
            t!("organization_page.auto_apply"),
            Some(t!("organization_page.auto_apply_hint")),
            |ui| widgets::toggle(ui, &mut o.auto_apply_to_new).changed(),
        );
        widgets::row_separator(ui);
        changed |= widgets::setting_row(
            ui,
            icons::INFO,
            t!("organization_page.show_new_count"),
            Some(t!("organization_page.show_new_count_hint")),
            |ui| widgets::toggle(ui, &mut o.show_new_count).changed(),
        );
        widgets::row_separator(ui);
        changed |= widgets::setting_row(
            ui,
            icons::INBOX,
            t!("organization_page.catch_all"),
            Some(t!("organization_page.catch_all_hint")),
            |ui| box_combo(ui, "fourre-tout", &boxes, &mut o.catch_all),
        );
        if changed {
            m.save();
        }
    });
    ui.add_space(12.0);

    // Types d'éléments.
    widgets::card(ui, |ui| {
        card_title(ui, icons::FILTER, t!("organization_page.types"));
        widgets::hint(ui, t!("organization_page.types_hint"));
        ui.add_space(6.0);
        if boxes.is_empty() {
            widgets::notice(
                ui,
                theme::ACCENT,
                icons::INFO,
                t!("organization_page.no_boxes"),
            );
            ui.add_space(6.0);
        }
        for (i, category) in Category::ALL.into_iter().enumerate() {
            if i > 0 {
                widgets::row_separator(ui);
            }
            category_row(m, ui, category, &boxes);
        }
    });
}

/// Liste déroulante des boîtes (« Aucune » comprise). Rend `true` si le choix change.
fn box_combo(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    boxes: &[(BoxId, String)],
    value: &mut Option<BoxId>,
) -> bool {
    let current = value
        .and_then(|id| boxes.iter().find(|(b, _)| *b == id))
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| t!("organization_page.no_box").to_owned());
    let before = *value;
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(widgets::truncate(&current, 24))
        .width(200.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, t!("organization_page.no_box"));
            for (id, name) in boxes {
                ui.selectable_value(value, Some(*id), name);
            }
        });
    *value != before
}

fn category_row(m: &mut Manager, ui: &mut egui::Ui, category: Category, boxes: &[(BoxId, String)]) {
    let setting = m.settings.organization.categories.get(&category).cloned();
    let extensions = extensions_of(setting.as_ref(), category);
    let open = m.drafts.open_categories.contains(&category);
    let mut target = setting.as_ref().and_then(|s| s.target);
    let custom = setting.as_ref().is_some_and(|s| s.extensions.is_some());

    let summary = if category.default_extensions().is_empty() {
        t!("organization_page.auto_detected").to_owned()
    } else {
        let mut s = match extensions.len() {
            1 => t!("organization_page.extension_count_one").to_owned(),
            n => t!("organization_page.extension_count", count = n),
        };
        if custom {
            s.push_str(" · ");
            s.push_str(t!("organization_page.customized"));
        }
        s
    };
    let mut toggle_open = false;
    let changed = widgets::setting_row(
        ui,
        category_icon(category),
        category_label(category),
        Some(&summary),
        |ui| {
            let chevron = if open {
                icons::CHEVRON_UP
            } else {
                icons::CHEVRON_DOWN
            };
            if widgets::icon_button(ui, chevron, t!("organization_page.details")).clicked() {
                toggle_open = true;
            }
            box_combo(ui, ("catégorie", category), boxes, &mut target)
        },
    );
    if toggle_open {
        if open {
            m.drafts.open_categories.remove(&category);
        } else {
            m.drafts.open_categories.insert(category);
        }
    }
    if changed {
        edit_category(m, category, |s| s.target = target);
    }
    if !open {
        return;
    }

    egui::Frame::new()
        .fill(theme::BG)
        .corner_radius(12)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if category.default_extensions().is_empty() && extensions.is_empty() {
                widgets::hint(ui, t!("organization_page.auto_detected_hint"));
            } else {
                extensions_editor(m, ui, category, &extensions, custom);
            }
            ui.add_space(6.0);
            let mut aliases = setting.as_ref().is_some_and(|s| s.include_aliases);
            if widgets::setting_row(
                ui,
                icons::LINK,
                t!("organization_page.include_aliases"),
                Some(t!("organization_page.include_aliases_hint")),
                |ui| widgets::toggle(ui, &mut aliases).changed(),
            ) {
                edit_category(m, category, |s| s.include_aliases = aliases);
            }
        });
}

fn extensions_editor(
    m: &mut Manager,
    ui: &mut egui::Ui,
    category: Category,
    extensions: &[String],
    custom: bool,
) {
    let mut removed = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for ext in extensions {
            if widgets::chip(ui, &format!(".{ext}")) {
                removed = Some(ext.clone());
            }
        }
    });
    if let Some(ext) = removed {
        let list: Vec<String> = extensions.iter().filter(|e| **e != ext).cloned().collect();
        edit_category(m, category, |s| s.extensions = Some(list));
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let draft = m.drafts.new_extension.entry(category).or_default();
        let response = ui.add(
            egui::TextEdit::singleline(draft)
                .hint_text(t!("organization_page.add_extension_hint"))
                .desired_width(180.0),
        );
        let entered = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let clicked = ui
            .button(format!(
                "{}  {}",
                icons::PLUS,
                t!("organization_page.add_extension")
            ))
            .clicked();
        if entered || clicked {
            let added = parse_extensions(draft);
            draft.clear();
            if !added.is_empty() {
                let mut list = extensions.to_vec();
                for ext in added {
                    if !list.contains(&ext) {
                        list.push(ext);
                    }
                }
                edit_category(m, category, |s| s.extensions = Some(list));
            }
        }
        if custom
            && ui
                .button(format!("{}  {}", icons::RESTORE, t!("common.reset")))
                .on_hover_text(t!("organization_page.reset_hint"))
                .clicked()
        {
            edit_category(m, category, |s| s.extensions = None);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_extensions_are_cleaned() {
        assert_eq!(
            parse_extensions(".JPG, png  webp;png"),
            vec!["jpg", "png", "webp"]
        );
        assert_eq!(parse_extensions("tar.gz ..  "), Vec::<String>::new());
        assert_eq!(parse_extensions("7z 001"), vec!["7z", "001"]);
    }

    #[test]
    fn default_extensions_are_used_until_changed() {
        let ext = extensions_of(None, Category::Image);
        assert!(ext.contains(&"png".to_owned()));
        let custom = CategorySetting {
            target: None,
            extensions: Some(vec!["png".into()]),
            include_aliases: false,
        };
        assert_eq!(extensions_of(Some(&custom), Category::Image), vec!["png"]);
        assert!(extensions_of(None, Category::Folder).is_empty());
    }

    #[test]
    fn every_category_has_a_label_and_an_icon() {
        for c in Category::ALL {
            assert!(!category_label(c).contains('.'), "{c:?}");
            assert!(!category_icon(c).is_empty());
        }
    }
}
