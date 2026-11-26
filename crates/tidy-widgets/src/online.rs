//! Outils communs aux widgets en ligne (lot I : météo, calendrier, chat IA) : réveil
//! de la fenêtre par les fils réseau, textes des erreurs, noms des jours et des mois.

use std::sync::Arc;

use egui::{RichText, Ui};
use jiff::civil::{DateTime, Weekday};
use tidy_integrations::NetError;
use tidy_integrations::task::Waker;
use tidy_ui::{icons, t, theme, widgets};

/// Réveille la fenêtre du widget (`ui` : celle qui dessine le widget).
pub(crate) fn waker(ui: &Ui) -> Waker {
    let ctx = ui.ctx().clone();
    let viewport = ui.ctx().viewport_id();
    Arc::new(move || ctx.request_repaint_of(viewport))
}

/// Erreur réseau, pour l'utilisateur.
pub(crate) fn net_error_text(e: &NetError) -> String {
    match e {
        NetError::Offline => t!("widgets2.err_offline").to_owned(),
        NetError::Timeout => t!("widgets2.err_timeout").to_owned(),
        NetError::Status(code) => t!("widgets2.err_status", code = code),
        NetError::Tls => t!("widgets2.err_tls").to_owned(),
        NetError::BadUrl => t!("widgets2.err_bad_url").to_owned(),
        NetError::Invalid(_) => t!("widgets2.err_invalid").to_owned(),
        NetError::Other(_) => t!("widgets2.err_other").to_owned(),
    }
}

/// Bandeau d'erreur avec un bouton « Réessayer ». `true` si on a cliqué.
pub(crate) fn error_notice(ui: &mut Ui, text: &str, retry: bool) -> bool {
    widgets::notice(ui, theme::WARNING, icons::WARNING, text);
    retry
        && ui
            .button(format!("{}  {}", icons::REFRESH, t!("widgets2.retry")))
            .clicked()
}

/// Nom court d'un jour (« lun. »).
pub(crate) fn weekday_short(wd: Weekday) -> &'static str {
    match wd {
        Weekday::Monday => t!("widgets2.wd_mon"),
        Weekday::Tuesday => t!("widgets2.wd_tue"),
        Weekday::Wednesday => t!("widgets2.wd_wed"),
        Weekday::Thursday => t!("widgets2.wd_thu"),
        Weekday::Friday => t!("widgets2.wd_fri"),
        Weekday::Saturday => t!("widgets2.wd_sat"),
        Weekday::Sunday => t!("widgets2.wd_sun"),
    }
}

/// Nom d'un mois (1 à 12).
pub(crate) fn month_name(month: i8) -> &'static str {
    match month {
        1 => t!("widgets2.month_1"),
        2 => t!("widgets2.month_2"),
        3 => t!("widgets2.month_3"),
        4 => t!("widgets2.month_4"),
        5 => t!("widgets2.month_5"),
        6 => t!("widgets2.month_6"),
        7 => t!("widgets2.month_7"),
        8 => t!("widgets2.month_8"),
        9 => t!("widgets2.month_9"),
        10 => t!("widgets2.month_10"),
        11 => t!("widgets2.month_11"),
        _ => t!("widgets2.month_12"),
    }
}

/// Heure « 09:30 ».
pub(crate) fn hm(dt: DateTime) -> String {
    format!("{:02}:{:02}", dt.hour(), dt.minute())
}

/// Petite ligne grise de pied de widget (source, mise à jour).
pub(crate) fn footer(ui: &mut Ui, text: impl Into<String>) {
    ui.label(
        RichText::new(text)
            .size(theme::SIZE_XS)
            .color(theme::TEXT_DIM),
    );
}

#[cfg(test)]
pub(crate) mod testing {
    //! Rendu d'un widget sans fenêtre (egui seul), pour vérifier qu'il se dessine.

    use crate::{Widget, WidgetCx};

    /// Dessine `w` (contenu puis réglages) `frames` fois. Rend le contexte des actions.
    pub fn render(w: &mut dyn Widget, frames: usize) -> WidgetCx {
        let ctx = egui::Context::default();
        tidy_ui::theme::apply(&ctx);
        let mut cx = WidgetCx::new(std::env::temp_dir());
        for _ in 0..frames {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(360.0, 900.0),
                )),
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                w.ui(ui, &mut cx);
                ui.separator();
                w.settings_ui(ui, &mut cx);
            });
            discard(output);
        }
        cx
    }

    /// Sans moteur de rendu, les textures produites sont jetées.
    pub fn discard(mut output: egui::FullOutput) {
        output.textures_delta.clear();
    }
}
