//! « Search Mini » (lot G) : un champ de recherche posé sur le bureau. Entrée (ou la
//! loupe) ouvre le lanceur de recherche avec le texte saisi
//! (`search/open`, traité par l'agent).

use egui::{Key, RichText};
use serde_json::{Value, json};
use tidy_ui::{icons, t, theme};

use crate::{Widget, WidgetAction, WidgetCx, WidgetKind};

/// Type du registre.
pub fn kind() -> WidgetKind {
    WidgetKind {
        id: "search_mini",
        name_key: "search.mini_name",
        icon: icons::SEARCH,
        default_size: [320.0, 44.0],
        create,
    }
}

fn create(_settings: &Value) -> Box<dyn Widget> {
    Box::new(SearchMini::default())
}

/// Champ de recherche du bureau. Sans réglage.
#[derive(Debug, Default)]
pub struct SearchMini {
    text: String,
}

impl SearchMini {
    /// Demande d'ouverture du lanceur avec le texte saisi, qui est vidé.
    fn submit(&mut self, cx: &mut WidgetCx) {
        cx.actions.push(WidgetAction::Module {
            module: "search".into(),
            action: "open".into(),
            data: json!({ "query": self.text.trim() }),
        });
        self.text.clear();
    }
}

impl Widget for SearchMini {
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx) {
        ui.horizontal_centered(|ui| {
            let icon = ui
                .add(
                    egui::Button::new(
                        RichText::new(icons::SEARCH)
                            .size(theme::SIZE_L)
                            .color(theme::ACCENT_2),
                    )
                    .frame(false),
                )
                .on_hover_text(t!("search.mini_open"))
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.text)
                    .hint_text(t!("search.mini_hint"))
                    .frame(egui::Frame::NONE)
                    .font(egui::FontId::proportional(theme::SIZE_L))
                    .desired_width(f32::INFINITY),
            );
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if enter || icon.clicked() {
                self.submit(cx);
            }
        });
    }

    fn save(&self) -> Value {
        json!({})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_asks_the_launcher() {
        let mut w = SearchMini {
            text: "  facture ".into(),
        };
        let mut cx = WidgetCx::new(std::env::temp_dir());
        w.submit(&mut cx);
        assert_eq!(
            cx.actions,
            [WidgetAction::Module {
                module: "search".into(),
                action: "open".into(),
                data: json!({ "query": "facture" }),
            }]
        );
        assert!(w.text.is_empty());

        // Dessin sans écran.
        let ctx = egui::Context::default();
        tidy_ui::theme::apply(&ctx);
        ctx.run_ui(egui::RawInput::default(), |ui| {
            w.ui(ui, &mut cx);
        })
        .drop_without_applying_deltas();
    }
}
