//! Page « Actions rapides » : double-clic pour masquer le bureau (et sa portée),
//! affichage au démarrage, création d'une boîte en dessinant, ouverture d'un ou deux
//! clics, accès rapide aux boîtes (maintenir ou basculer) et leurs raccourcis.

use tidy_core::settings::{QuickAccessMode, QuickHideScope};
use tidy_ui::{icons, t, widgets};

use super::Manager;
use super::pages::{card_title, page_header};
use super::shortcuts::{self, Slot};

pub fn show(m: &mut Manager, ui: &mut egui::Ui) {
    page_header(
        ui,
        icons::QUICK_ACTIONS,
        t!("manager.quick_actions"),
        t!("quick_actions_page.intro"),
    );

    // Masquage rapide.
    widgets::card(ui, |ui| {
        card_title(ui, icons::EYE_OFF, t!("quick_actions_page.hide_title"));
        let q = &mut m.settings.quick_actions;
        let mut changed = false;
        changed |= widgets::setting_row(
            ui,
            icons::POINTER,
            t!("quick_actions_page.double_click_hide"),
            Some(t!("quick_actions_page.double_click_hide_hint")),
            |ui| widgets::toggle(ui, &mut q.double_click_to_hide).changed(),
        );
        widgets::row_separator(ui);
        let enabled = q.double_click_to_hide;
        changed |= widgets::setting_row(
            ui,
            icons::LAYOUT,
            t!("quick_actions_page.hide_scope"),
            Some(t!("quick_actions_page.hide_scope_hint")),
            |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    let boxes = widgets::choice_value(
                        ui,
                        &mut q.hide_scope,
                        QuickHideScope::BoxesOnly,
                        t!("quick_actions_page.scope_boxes"),
                    );
                    let all = widgets::choice_value(
                        ui,
                        &mut q.hide_scope,
                        QuickHideScope::Everything,
                        t!("quick_actions_page.scope_everything"),
                    );
                    boxes.changed() || all.changed()
                })
                .inner
            },
        );
        widgets::row_separator(ui);
        changed |= widgets::setting_row(
            ui,
            icons::EYE,
            t!("quick_actions_page.show_on_start"),
            Some(t!("quick_actions_page.show_on_start_hint")),
            |ui| widgets::toggle(ui, &mut q.show_on_start).changed(),
        );
        if changed {
            m.save();
        }
        widgets::row_separator(ui);
        shortcuts::row(m, ui, Slot::ToggleDesktop);
    });
    ui.add_space(12.0);

    // Bureau : créer, ouvrir.
    widgets::card(ui, |ui| {
        card_title(ui, icons::MONITOR, t!("quick_actions_page.desktop_title"));
        let q = &mut m.settings.quick_actions;
        let mut changed = false;
        changed |= widgets::setting_row(
            ui,
            icons::DRAW_BOX,
            t!("quick_actions_page.draw_to_create"),
            Some(t!("quick_actions_page.draw_to_create_hint")),
            |ui| widgets::toggle(ui, &mut q.draw_to_create).changed(),
        );
        widgets::row_separator(ui);
        changed |= widgets::setting_row(
            ui,
            icons::QUICK_ACTIONS,
            t!("quick_actions_page.open_with"),
            Some(t!("quick_actions_page.open_with_hint")),
            |ui| {
                let double = widgets::choice_value(
                    ui,
                    &mut q.single_click_open,
                    false,
                    t!("quick_actions_page.double_click"),
                );
                let single = widgets::choice_value(
                    ui,
                    &mut q.single_click_open,
                    true,
                    t!("quick_actions_page.single_click"),
                );
                double.changed() || single.changed()
            },
        );
        if changed {
            m.save();
        }
    });
    ui.add_space(12.0);

    // Accès rapide aux boîtes.
    widgets::card(ui, |ui| {
        card_title(
            ui,
            icons::BOXES,
            t!("quick_actions_page.quick_access_title"),
        );
        widgets::hint(ui, t!("quick_actions_page.quick_access_hint"));
        ui.add_space(4.0);
        shortcuts::row(m, ui, Slot::QuickAccess);
        widgets::row_separator(ui);
        let mode = &mut m.settings.hotkeys.quick_access_mode;
        let changed = widgets::setting_row(
            ui,
            icons::HAND,
            t!("quick_actions_page.quick_access_mode"),
            Some(match mode {
                QuickAccessMode::Hold => t!("quick_actions_page.mode_hold_hint"),
                QuickAccessMode::Toggle => t!("quick_actions_page.mode_toggle_hint"),
            }),
            |ui| {
                let toggle = widgets::choice_value(
                    ui,
                    mode,
                    QuickAccessMode::Toggle,
                    t!("quick_actions_page.mode_toggle"),
                );
                let hold = widgets::choice_value(
                    ui,
                    mode,
                    QuickAccessMode::Hold,
                    t!("quick_actions_page.mode_hold"),
                );
                toggle.changed() || hold.changed()
            },
        );
        if changed {
            m.save();
        }
        widgets::row_separator(ui);
        widgets::hint(ui, t!("quick_actions_page.exclusions_hint"));
    });
}
