//! Contenu des pages de la fenêtre principale.

use tidy_core::settings::{DesktopMode, Language};
use tidy_platform::LoginState;
use tidy_ui::{Lang, icons, t, theme, widgets};

use super::{Manager, Page};
use crate::ipc::Request;

pub fn show(m: &mut Manager, ui: &mut egui::Ui) {
    ui.set_max_width(ui.available_width().min(760.0));
    if let Some(error) = m.error.clone() {
        widgets::card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(theme::DANGER, format!("{}  {error}", icons::WARNING));
                if ui.small_button(icons::CLOSE).clicked() {
                    m.error = None;
                }
            });
        });
        ui.add_space(12.0);
    }
    match m.page {
        Page::Home => home(m, ui),
        Page::Options => options(m, ui),
        Page::Diagnostic => diagnostic(m, ui),
        page => coming_soon(ui, page),
    }
}

fn mode_label(mode: DesktopMode) -> &'static str {
    match mode {
        DesktopMode::Off => t!("options.mode_off"),
        DesktopMode::Overlay => t!("options.mode_overlay"),
        DesktopMode::Managed => t!("options.mode_managed"),
    }
}

fn home(m: &mut Manager, ui: &mut egui::Ui) {
    widgets::page_title(ui, t!("home.welcome"));
    ui.add_space(4.0);
    widgets::hint(ui, t!("home.intro"));
    ui.add_space(16.0);
    widgets::card(ui, |ui| {
        ui.label(t!(
            "home.desktop_mode",
            mode = mode_label(m.settings.effective_desktop_mode())
        ));
    });
}

fn coming_soon(ui: &mut egui::Ui, page: Page) {
    widgets::page_title(ui, format!("{}  {}", page.icon(), page.label()));
    ui.add_space(16.0);
    widgets::card(ui, |ui| {
        widgets::section_title(ui, t!("common.coming_soon"));
        widgets::hint(ui, t!("common.coming_soon_hint"));
    });
}

fn options(m: &mut Manager, ui: &mut egui::Ui) {
    widgets::page_title(ui, t!("manager.options"));
    ui.add_space(16.0);

    // Langue.
    widgets::card(ui, |ui| {
        widgets::section_title(
            ui,
            format!("{}  {}", icons::LANGUAGE, t!("options.language")),
        );
        ui.horizontal_wrapped(|ui| {
            let mut lang = m.settings.language;
            let mut changed = false;
            changed |= widgets::choice_value(
                ui,
                &mut lang,
                Language::System,
                t!("options.language_system"),
            )
            .changed();
            for (value, l) in [(Language::Fr, Lang::Fr), (Language::En, Lang::En)] {
                changed |= widgets::choice_value(ui, &mut lang, value, l.native_name()).changed();
            }
            if changed {
                m.settings.language = lang;
                m.save();
            }
        });
    });
    ui.add_space(12.0);

    // Ouverture de session.
    widgets::card(ui, |ui| {
        let state = tidy_platform::current().login_item();
        let mut on = matches!(state, LoginState::On | LoginState::NeedsApproval);
        let available = !matches!(state, LoginState::Unavailable(_));
        if ui
            .add_enabled(
                available,
                egui::Checkbox::new(&mut on, t!("options.launch_at_login")),
            )
            .changed()
        {
            m.settings.launch_at_login = on;
            m.save();
        }
        match state {
            LoginState::NeedsApproval => {
                widgets::hint(ui, t!("options.launch_at_login_approval"));
            }
            LoginState::Unavailable(why) => {
                widgets::hint(ui, why);
            }
            _ => {}
        }
    });
    ui.add_space(12.0);

    // Mode du bureau.
    widgets::card(ui, |ui| {
        widgets::section_title(
            ui,
            format!("{}  {}", icons::MONITOR, t!("options.desktop_mode")),
        );
        let current = m.settings.effective_desktop_mode();
        ui.horizontal_wrapped(|ui| {
            // Le mode géré (macOS) attend que Tidy dessine les icônes hors boîte
            // (ADR 011) : il n'est plus proposé pour l'instant.
            for mode in [DesktopMode::Off, DesktopMode::Overlay] {
                if widgets::choice(ui, current == mode, mode_label(mode)).clicked()
                    && current != mode
                {
                    m.settings.desktop_mode = mode;
                    m.save();
                }
            }
        });
        widgets::hint(
            ui,
            match current {
                DesktopMode::Off => t!("options.mode_off_hint"),
                DesktopMode::Overlay => t!("options.mode_overlay_hint"),
                DesktopMode::Managed => t!("options.mode_managed_hint"),
            },
        );
    });
    ui.add_space(12.0);

    // Raccourcis (lecture seule pour l'instant : l'éditeur arrive en vague 1).
    widgets::card(ui, |ui| {
        widgets::section_title(
            ui,
            format!("{}  {}", icons::KEYBOARD, t!("options.hotkeys")),
        );
        let h = &m.settings.hotkeys;
        egui::Grid::new("raccourcis")
            .num_columns(2)
            .spacing([24.0, 8.0])
            .show(ui, |ui| {
                for (label, value) in [
                    (t!("options.hotkey_search"), &h.search),
                    (t!("options.hotkey_toggle_desktop"), &h.toggle_desktop),
                    (t!("options.hotkey_quick_access"), &h.quick_access),
                    (t!("options.hotkey_main_window"), &h.main_window),
                ] {
                    ui.label(label);
                    match value {
                        Some(v) => ui.monospace(v),
                        None => ui.label(widgets::dim(t!("common.none"))),
                    };
                    ui.end_row();
                }
            });
    });
}

fn diagnostic(m: &mut Manager, ui: &mut egui::Ui) {
    widgets::page_title(ui, t!("manager.diagnostic"));
    ui.add_space(16.0);
    let root = m.store.root().to_owned();
    let logs = m.store.logs_dir();
    widgets::card(ui, |ui| {
        egui::Grid::new("diagnostic")
            .num_columns(2)
            .spacing([24.0, 8.0])
            .show(ui, |ui| {
                ui.label(t!("diagnostic.version"));
                ui.monospace(env!("CARGO_PKG_VERSION"));
                ui.end_row();
                ui.label(t!("diagnostic.data_dir"));
                path_label(ui, &root);
                ui.end_row();
                ui.label(t!("diagnostic.logs_dir"));
                path_label(ui, &logs);
                ui.end_row();
                ui.label(t!("diagnostic.finder_icons"));
                let hidden = m.status.as_ref().is_some_and(|s| s.finder_hidden);
                ui.label(if hidden {
                    t!("diagnostic.finder_hidden")
                } else {
                    t!("diagnostic.finder_visible")
                });
                ui.end_row();
            });
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(format!(
                    "{}  {}",
                    icons::FOLDER_OPEN,
                    t!("diagnostic.open_data")
                ))
                .clicked()
                && let Err(e) = tidy_platform::current().open(&root)
            {
                m.error = Some(format!("{e:#}"));
            }
            if ui
                .button(format!(
                    "{}  {}",
                    icons::DOCUMENT,
                    t!("diagnostic.open_logs")
                ))
                .clicked()
            {
                let _ = std::fs::create_dir_all(&logs);
                if let Err(e) = tidy_platform::current().open(&logs) {
                    m.error = Some(format!("{e:#}"));
                }
            }
            if m.status.as_ref().is_some_and(|s| s.finder_hidden)
                && widgets::primary(ui, t!("diagnostic.restore_finder")).clicked()
            {
                m.request(Request::RestoreFinder);
            }
        });
    });
    ui.add_space(12.0);
    widgets::card(ui, |ui| {
        widgets::section_title(
            ui,
            format!("{}  {}", icons::MONITOR, t!("diagnostic.screens")),
        );
        for s in tidy_platform::current().screens() {
            ui.label(format!(
                "{} {} · {}×{} pt · ×{} · {}",
                s.name,
                if s.primary { icons::STAR } else { "" },
                s.frame.width,
                s.frame.height,
                s.scale,
                s.id.0
            ));
        }
    });
}

/// Chemin en police fixe, coupé s'il est trop long (entier au survol).
fn path_label(ui: &mut egui::Ui, path: &std::path::Path) {
    let text = path.display().to_string();
    ui.add(egui::Label::new(egui::RichText::new(&text).monospace()).truncate())
        .on_hover_text(text);
}
