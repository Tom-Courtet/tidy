//! Pages simples de la fenêtre principale (Accueil, Options, Diagnostic), aiguillage
//! vers les autres et modale de confirmation.

use tidy_core::DesktopCommand;
use tidy_core::settings::{DesktopMode, Language};
use tidy_platform::LoginState;
use tidy_ui::{Lang, icons, t, theme, widgets};

use super::shortcuts::{self, Slot};
use super::{Confirm, Manager, Page};
use crate::ipc::Request;

/// Largeur maximale du contenu : les lignes de réglage restent lisibles.
const MAX_WIDTH: f32 = 780.0;

pub fn show(m: &mut Manager, ui: &mut egui::Ui) {
    ui.set_max_width(ui.available_width().min(MAX_WIDTH));
    banners(m, ui);
    match m.page {
        Page::Home => home(m, ui),
        Page::Boxes => super::boxes::show(m, ui),
        Page::Organization => super::organization::show(m, ui),
        Page::QuickActions => super::quick_actions::show(m, ui),
        Page::Layouts => super::layouts::show(m, ui),
        Page::Widgets => super::widgets_page::show(m, ui),
        Page::Options => options(m, ui),
        Page::Diagnostic => diagnostic(m, ui),
        page => coming_soon(ui, page),
    }
    ui.add_space(24.0);
}

/// Erreur (rouge) et message de réussite (lime) en haut de la page.
pub fn banners(m: &mut Manager, ui: &mut egui::Ui) {
    if let Some(error) = m.error.clone() {
        let response = widgets::notice(ui, theme::DANGER, icons::WARNING, &error)
            .on_hover_text(t!("manager.dismiss"));
        if response.interact(egui::Sense::click()).clicked() {
            m.error = None;
        }
        ui.add_space(12.0);
    }
    if let Some((text, _)) = &m.notice {
        widgets::notice(ui, theme::ACCENT_2, icons::DONE, text);
        ui.add_space(12.0);
    }
}

/// Titre de page et sa phrase d'introduction.
pub fn page_header(ui: &mut egui::Ui, icon: &str, title: &str, intro: &str) {
    widgets::page_title(ui, format!("{icon}  {title}"));
    ui.add_space(2.0);
    widgets::hint(ui, intro);
    ui.add_space(18.0);
}

/// Titre d'une carte, avec son icône.
pub fn card_title(ui: &mut egui::Ui, icon: &str, title: &str) {
    widgets::section_title(ui, format!("{icon}  {title}"));
    ui.add_space(6.0);
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
    ui.add_space(18.0);
    widgets::card(ui, |ui| {
        ui.label(t!(
            "home.desktop_mode",
            mode = mode_label(m.settings.effective_desktop_mode())
        ));
        ui.label(widgets::dim(t!(
            "home.box_count",
            count = m.desktop.boxes.len()
        )));
    });
    ui.add_space(12.0);
    // Raccourcis vers les pages principales.
    let links = [
        (Page::Boxes, t!("home.link_boxes")),
        (Page::Organization, t!("home.link_organization")),
        (Page::QuickActions, t!("home.link_quick_actions")),
    ];
    let gap = 12.0;
    let width = (ui.available_width() - gap * 2.0) / 3.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (page, body) in links {
            if widgets::choice_card(ui, false, width, page.icon(), page.label(), body).clicked() {
                m.page = page;
            }
        }
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

/// Choix de la langue (Options et assistant).
pub fn language_choices(m: &mut Manager, ui: &mut egui::Ui) {
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
}

/// Ligne « Lancer Tidy à l'ouverture de session » (Options et assistant).
pub fn launch_at_login_row(m: &mut Manager, ui: &mut egui::Ui) {
    let state = tidy_platform::current().login_item();
    let available = !matches!(state, LoginState::Unavailable(_));
    let hint = match &state {
        LoginState::NeedsApproval => Some(t!("options.launch_at_login_approval").to_owned()),
        LoginState::Unavailable(why) => Some(why.clone()),
        _ => None,
    };
    let mut on = m.settings.launch_at_login;
    let changed = widgets::setting_row(
        ui,
        icons::LOGIN,
        t!("options.launch_at_login"),
        hint.as_deref(),
        |ui| ui.add_enabled_ui(available, |ui| widgets::toggle(ui, &mut on)),
    )
    .inner
    .changed();
    if changed {
        m.settings.launch_at_login = on;
        m.save();
    }
}

fn options(m: &mut Manager, ui: &mut egui::Ui) {
    page_header(
        ui,
        icons::OPTIONS,
        t!("manager.options"),
        t!("options.intro"),
    );

    widgets::card(ui, |ui| {
        card_title(ui, icons::LANGUAGE, t!("options.language"));
        language_choices(m, ui);
        widgets::row_separator(ui);
        launch_at_login_row(m, ui);
    });
    ui.add_space(12.0);

    // Mode du bureau.
    widgets::card(ui, |ui| {
        card_title(ui, icons::MONITOR, t!("options.desktop_mode"));
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

    // Raccourcis globaux, avec détection des conflits.
    widgets::card(ui, |ui| {
        card_title(ui, icons::KEYBOARD, t!("options.hotkeys"));
        widgets::hint(ui, t!("options.hotkeys_hint"));
        ui.add_space(4.0);
        let slots = [
            Slot::Search,
            Slot::ToggleDesktop,
            Slot::QuickAccess,
            Slot::MainWindow,
        ];
        for (i, slot) in slots.into_iter().enumerate() {
            if i > 0 {
                widgets::row_separator(ui);
            }
            shortcuts::row(m, ui, slot);
        }
    });
    ui.add_space(12.0);

    widgets::card(ui, |ui| {
        let clicked = widgets::setting_row(
            ui,
            icons::ROCKET,
            t!("options.rerun_onboarding"),
            Some(t!("options.rerun_onboarding_hint")),
            |ui| ui.button(t!("options.rerun_onboarding_button")).clicked(),
        );
        if clicked {
            m.onboarding = Some(super::onboarding::Wizard::default());
        }
    });
}

fn diagnostic(m: &mut Manager, ui: &mut egui::Ui) {
    page_header(
        ui,
        icons::DIAGNOSTIC,
        t!("manager.diagnostic"),
        t!("diagnostic.intro"),
    );
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
            if widgets::primary(
                ui,
                format!("{}  {}", icons::CLIPBOARD, t!("diagnostic.copy_report")),
            )
            .on_hover_text(t!("diagnostic.copy_report_hint"))
            .clicked()
            {
                let text = super::report::build(m);
                ui.ctx().copy_text(text);
                m.show_notice(t!("diagnostic.report_copied"));
            }
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
        card_title(ui, icons::MONITOR, t!("diagnostic.screens"));
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

/// Modale de confirmation en attente, et son action.
pub fn confirmation(m: &mut Manager, ctx: &egui::Context) {
    let Some(confirm) = m.confirm else {
        return;
    };
    let answer = match confirm {
        Confirm::DeleteBox(id) => {
            let name = m
                .desktop
                .find_box(id)
                .map(|b| super::boxes::display_name(&b.name))
                .unwrap_or_default();
            widgets::confirm(
                ctx,
                "supprimer-boîte",
                &t!("boxes_page.delete_title", name = name),
                t!("boxes_page.delete_body"),
                t!("common.delete"),
                true,
            )
        }
        Confirm::ApplyToAll => widgets::confirm(
            ctx,
            "ranger-tout",
            t!("organization_page.apply_all_title"),
            t!("organization_page.apply_all_body"),
            t!("organization_page.apply_all"),
            false,
        ),
        Confirm::UndoOrganize => widgets::confirm(
            ctx,
            "annuler-rangement",
            t!("organization_page.undo_title"),
            t!("organization_page.undo_body"),
            t!("organization_page.undo"),
            false,
        ),
    };
    let Some(yes) = answer else {
        return;
    };
    m.confirm = None;
    if !yes {
        return;
    }
    match confirm {
        Confirm::DeleteBox(id) => {
            if m.send(DesktopCommand::RemoveBox { id }) {
                m.show_notice(t!("boxes_page.deleted"));
            }
            m.drafts.box_names.remove(&id);
        }
        Confirm::ApplyToAll => {
            if m.request(Request::ApplyRules { all: true }) {
                m.show_notice(t!("organization_page.applied"));
            }
        }
        Confirm::UndoOrganize => {
            if m.request(Request::UndoOrganize) {
                m.show_notice(t!("organization_page.undone"));
            }
        }
    }
}
