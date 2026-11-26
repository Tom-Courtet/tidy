//! Aperçu des widgets sans l'hôte du bureau : chaque type du registre dans un cadre de
//! sa taille par défaut, avec ses réglages en dessous.
//!
//! ```sh
//! cargo run -p tidy-widgets --example apercu
//! cargo run -p tidy-widgets --example apercu -- weather calendar
//! ```
//!
//! Les widgets en ligne se comportent comme sur le bureau : la météo interroge
//! Open-Meteo, le chat IA lit sa clé dans le gestionnaire d'identifiants à l'envoi.
//! Variables utiles : `APERCU_VILLE` (une ville pour la météo, par exemple « Lyon »)
//! et `APERCU_ICS` (une adresse ICS pour le calendrier).

use tidy_widgets::{Widget, WidgetCx, WidgetKind, kinds};

struct Preview {
    widgets: Vec<(WidgetKind, Box<dyn Widget>)>,
    cx: WidgetCx,
}

fn settings_for(id: &str) -> serde_json::Value {
    match id {
        "weather" => match std::env::var("APERCU_VILLE") {
            // Coordonnées données à la main : l'aperçu ne cherche pas la ville.
            Ok(name) => serde_json::json!({
                "places": [{ "name": name, "latitude": 45.7485, "longitude": 4.8467 }]
            }),
            Err(_) => serde_json::Value::Null,
        },
        "calendar" => match std::env::var("APERCU_ICS") {
            Ok(url) => serde_json::json!({
                "calendars": [{ "name": "Aperçu", "url": url, "color": [198, 255, 52], "enabled": true }]
            }),
            Err(_) => serde_json::Value::Null,
        },
        _ => serde_json::Value::Null,
    }
}

impl eframe::App for Preview {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut next = None;
        egui::ScrollArea::both().show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for (kind, widget) in &mut self.widgets {
                    ui.vertical(|ui| {
                        ui.label(tidy_ui::i18n::tr(kind.name_key));
                        egui::Frame::new()
                            .fill(tidy_ui::theme::SURFACE)
                            .stroke(egui::Stroke::new(1.0, tidy_ui::theme::BORDER))
                            .corner_radius(tidy_ui::theme::CARD_RADIUS)
                            .inner_margin(12)
                            .show(ui, |ui| {
                                ui.set_width(kind.default_size[0] - 24.0);
                                ui.set_height(kind.default_size[1] - 24.0);
                                widget.ui(ui, &mut self.cx);
                            });
                        ui.add_space(8.0);
                        egui::Frame::new()
                            .fill(tidy_ui::theme::SURFACE)
                            .inner_margin(12)
                            .show(ui, |ui| {
                                ui.set_width(kind.default_size[0] - 24.0);
                                widget.settings_ui(ui, &mut self.cx);
                            });
                    });
                    if let Some(after) = widget.repaint_after() {
                        next = Some(next.map_or(after, |n: std::time::Duration| n.min(after)));
                    }
                }
            });
        });
        for action in self.cx.actions.drain(..) {
            println!("action : {action:?}");
        }
        if let Some(after) = next {
            ui.ctx().request_repaint_after(after);
        }
    }
}

fn main() -> eframe::Result {
    let wanted: Vec<String> = std::env::args().skip(1).collect();
    let widgets = kinds()
        .into_iter()
        .filter(|k| wanted.is_empty() || wanted.iter().any(|w| w == k.id))
        .map(|k| {
            let w = (k.create)(&settings_for(k.id));
            (k, w)
        })
        .collect();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 900.0])
            .with_position([40.0, 40.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Aperçu des widgets",
        options,
        Box::new(|cc| {
            tidy_ui::theme::apply(&cc.egui_ctx);
            Ok(Box::new(Preview {
                widgets,
                cx: WidgetCx::new(std::env::temp_dir()),
            }))
        }),
    )
}
