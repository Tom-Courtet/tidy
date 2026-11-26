//! Assistant de premier lancement, en trois étapes :
//! 1. « Démarrage rapide » (Tidy range le bureau tout de suite) ou « Personnaliser »
//!    (aucun rangement) ;
//! 2. langue et lancement à l'ouverture de session ; sous Windows, installation si
//!    l'exécutable ne tourne pas depuis son dossier d'installation ; sous macOS,
//!    autorisations demandées (lot K) ;
//! 3. récapitulatif, puis `onboarding_done = true`.
//!
//! Le démarrage rapide demande `ApplyRules { all: true }` à l'agent, qui crée les
//! boîtes par défaut. Pas d'étape « mode géré » (ADR 011).

use egui::RichText;
use tidy_ui::theme::{self, Typo};
use tidy_ui::{icons, t, widgets};

use super::pages::{self, banners};
use super::{Manager, Page};
use crate::ipc::Request;

/// Nombre d'étapes de l'assistant.
const STEPS: usize = 3;
/// Largeur du contenu de l'assistant.
const WIDTH: f32 = 640.0;

/// Choix du premier écran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Start {
    #[default]
    Quick,
    Custom,
}

#[derive(Debug, Clone, Default)]
pub struct Wizard {
    pub step: usize,
    pub start: Start,
}

pub fn show(m: &mut Manager, ui: &mut egui::Ui) {
    let Some(mut wizard) = m.onboarding.clone() else {
        return;
    };
    let width = ui.available_width().min(WIDTH);
    let margin = ((ui.available_width() - width) / 2.0).max(0.0);
    let mut finish = false;
    ui.horizontal(|ui| {
        ui.add_space(margin);
        ui.vertical(|ui| {
            ui.set_width(width);
            banners(m, ui);
            widgets::step_dots(ui, STEPS, wizard.step);
            ui.add_space(20.0);
            match wizard.step {
                0 => choose_start(ui, &mut wizard, width),
                1 => preferences(m, ui),
                _ => summary(ui, &wizard),
            }
            ui.add_space(24.0);
            ui.horizontal(|ui| {
                if wizard.step > 0
                    && ui
                        .button(format!("{}  {}", icons::BACK, t!("onboarding.back")))
                        .clicked()
                {
                    wizard.step -= 1;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let last = wizard.step + 1 == STEPS;
                    let label = if !last {
                        format!("{}  {}", t!("onboarding.next"), icons::NEXT)
                    } else if wizard.start == Start::Quick {
                        format!("{}  {}", icons::AUTO_TIDY, t!("onboarding.finish_quick"))
                    } else {
                        format!("{}  {}", icons::OK, t!("onboarding.finish_custom"))
                    };
                    if widgets::primary(ui, label).clicked() {
                        if last {
                            finish = true;
                        } else {
                            wizard.step += 1;
                        }
                    }
                });
            });
        });
    });
    if finish {
        complete(m, wizard.start);
    } else {
        m.onboarding = Some(wizard);
    }
}

fn choose_start(ui: &mut egui::Ui, wizard: &mut Wizard, width: f32) {
    ui.horizontal(|ui| {
        widgets::logo(ui, 44.0);
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(t!("onboarding.welcome")).title().size(24.0));
            ui.label(widgets::dim(t!("onboarding.welcome_hint")));
        });
    });
    ui.add_space(22.0);
    let gap = 14.0;
    let card = (width - gap) / 2.0;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        if widgets::choice_card(
            ui,
            wizard.start == Start::Quick,
            card,
            icons::ZAP,
            t!("onboarding.quick"),
            t!("onboarding.quick_body"),
        )
        .clicked()
        {
            wizard.start = Start::Quick;
        }
        if widgets::choice_card(
            ui,
            wizard.start == Start::Custom,
            card,
            icons::OPTIONS,
            t!("onboarding.custom"),
            t!("onboarding.custom_body"),
        )
        .clicked()
        {
            wizard.start = Start::Custom;
        }
    });
    ui.add_space(12.0);
    widgets::notice(
        ui,
        theme::ACCENT,
        icons::SHIELD,
        t!("onboarding.files_safe"),
    );
}

fn preferences(m: &mut Manager, ui: &mut egui::Ui) {
    ui.label(
        RichText::new(t!("onboarding.preferences"))
            .title()
            .size(22.0),
    );
    ui.label(widgets::dim(t!("onboarding.preferences_hint")));
    ui.add_space(18.0);
    widgets::card(ui, |ui| {
        pages::card_title(ui, icons::LANGUAGE, t!("options.language"));
        pages::language_choices(m, ui);
        widgets::row_separator(ui);
        pages::launch_at_login_row(m, ui);
    });
    ui.add_space(12.0);
    // Lot K : installation (Windows, exécutable lancé depuis Téléchargements…) et
    // autorisations demandées par macOS.
    super::finish::install_offer(m, ui);
    super::finish::permissions(m, ui);
}

fn summary(ui: &mut egui::Ui, wizard: &Wizard) {
    ui.label(RichText::new(t!("onboarding.ready")).title().size(22.0));
    ui.add_space(4.0);
    widgets::hint(
        ui,
        match wizard.start {
            Start::Quick => t!("onboarding.ready_quick"),
            Start::Custom => t!("onboarding.ready_custom"),
        },
    );
    ui.add_space(18.0);
    widgets::card(ui, |ui| {
        pages::card_title(ui, icons::INFO, t!("onboarding.tips"));
        for (icon, tip) in [
            (icons::POINTER, t!("onboarding.tip_double_click")),
            (icons::DRAW_BOX, t!("onboarding.tip_draw")),
            (icons::KEYBOARD, t!("onboarding.tip_hotkeys")),
            (icons::UNDO, t!("onboarding.tip_undo")),
        ] {
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).color(theme::ACCENT_2));
                ui.add(egui::Label::new(tip).wrap());
            });
        }
    });
}

/// Fin de l'assistant : réglage enregistré, puis rangement si démarrage rapide.
fn complete(m: &mut Manager, start: Start) {
    m.settings.onboarding_done = true;
    m.save();
    m.onboarding = None;
    match start {
        Start::Quick => {
            m.page = Page::Home;
            if m.request(Request::ApplyRules { all: true }) {
                m.show_notice(t!("onboarding.done_quick"));
            }
        }
        Start::Custom => {
            m.page = Page::Boxes;
            m.show_notice(t!("onboarding.done_custom"));
        }
    }
}
