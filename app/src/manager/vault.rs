//! Carte « Boîte privée » de la page Boîtes (lot F) : état du coffre, réglages du
//! verrouillage automatique et de l'opacité.
//!
//! Aucun mot de passe ne passe par ici : les boutons demandent au processus du bureau
//! (`Request::Module { module: "vault" }`) d'ouvrir ses formulaires, sur le bureau.

use egui::Slider;
use tidy_core::model::BoxKind;
use tidy_ui::{icons, t, theme, widgets};
use tidy_vault::FileVault;

use super::Manager;
use super::pages::card_title;
use crate::ipc::Request;

/// Demande au bureau (`create`, `show`, `lock`, `change_password`, `forgot`, `delete`).
fn ask(m: &mut Manager, action: &str) {
    let sent = m.request(Request::Module {
        module: "vault".into(),
        action: action.into(),
        data: serde_json::Value::Null,
    });
    if sent {
        m.show_notice(t!("vault.sent"));
    }
}

pub fn show(m: &mut Manager, ui: &mut egui::Ui) {
    ui.add_space(20.0);
    widgets::card(ui, |ui| {
        card_title(ui, icons::LOCK, t!("vault.card_title"));
        widgets::hint(ui, t!("vault.card_hint"));
        ui.add_space(8.0);
        let exists = FileVault::exists_in(&m.store.root().join("vault"));
        let has_box = m
            .desktop
            .boxes
            .iter()
            .any(|b| matches!(b.kind, BoxKind::Private));
        ui.horizontal_wrapped(|ui| {
            if exists {
                ui.label(egui::RichText::new(icons::SHIELD).color(theme::ACCENT_2));
                ui.label(t!("vault.card_exists"));
            } else {
                ui.label(widgets::dim(t!("vault.card_none")));
            }
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if !exists {
                if widgets::primary(ui, format!("{}  {}", icons::PLUS, t!("vault.card_create")))
                    .clicked()
                {
                    ask(m, "create");
                }
                return;
            }
            if !has_box && widgets::primary(ui, t!("vault.card_show")).clicked()
                || has_box && ui.button(t!("vault.card_show")).clicked()
            {
                ask(m, "show");
            }
            if ui
                .button(format!("{}  {}", icons::LOCK, t!("vault.card_lock")))
                .clicked()
            {
                ask(m, "lock");
            }
            if ui
                .button(format!("{}  {}", icons::KEY, t!("vault.card_change")))
                .clicked()
            {
                ask(m, "change_password");
            }
            if ui.button(t!("vault.card_forgot")).clicked() {
                ask(m, "forgot");
            }
            if widgets::danger(ui, t!("vault.card_delete")).clicked() {
                ask(m, "delete");
            }
        });
        widgets::row_separator(ui);

        let v = &mut m.settings.vault;
        let mut save = false;
        let mut save_later = false;
        save |= widgets::setting_row(
            ui,
            icons::TIMER,
            t!("vault.auto_lock"),
            Some(t!("vault.auto_lock_hint")),
            |ui| {
                let mut changed = false;
                let mut minutes = v.auto_lock_minutes.clamp(
                    *tidy_core::settings::vault::VaultSettings::MINUTES.start(),
                    *tidy_core::settings::vault::VaultSettings::MINUTES.end(),
                );
                changed |= widgets::toggle(ui, &mut v.auto_lock).changed();
                let slider = ui.add_enabled(
                    v.auto_lock,
                    Slider::new(
                        &mut minutes,
                        tidy_core::settings::vault::VaultSettings::MINUTES,
                    )
                    .logarithmic(true)
                    .suffix(" min"),
                );
                if slider.changed() {
                    v.auto_lock_minutes = minutes;
                    save_later = true;
                }
                changed
            },
        );
        widgets::row_separator(ui);
        save |= widgets::setting_row(ui, icons::POWER, t!("vault.lock_on_sleep"), None, |ui| {
            widgets::toggle(ui, &mut v.lock_on_sleep).changed()
        });
        widgets::row_separator(ui);
        save |= widgets::setting_row(
            ui,
            icons::LOGIN,
            t!("vault.lock_on_session"),
            Some(t!("vault.lock_on_session_hint")),
            |ui| widgets::toggle(ui, &mut v.lock_on_session_lock).changed(),
        );
        widgets::row_separator(ui);
        widgets::setting_row(
            ui,
            icons::OPACITY,
            t!("vault.opacity"),
            Some(t!("vault.opacity_hint")),
            |ui| {
                let mut percent = (v.opacity() * 100.0).round() as u32;
                if ui
                    .add(Slider::new(&mut percent, 0..=100).suffix(" %"))
                    .changed()
                {
                    v.locked_opacity = percent as f32 / 100.0;
                    save_later = true;
                }
            },
        );
        if save {
            m.save();
        } else if save_later {
            m.save_later();
        }
    });
}
