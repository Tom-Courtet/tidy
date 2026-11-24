//! Petits widgets partagés (repris de Tappy et Soundy) : logo, textes d'aide, boutons
//! pleins, choix, pastilles de couleur, confirmation, cartes.

use egui::{
    Align2, Button, Color32, CornerRadius, Id, Key, Label, Modifiers, Response, RichText, Sense,
    Stroke, StrokeKind, Ui,
};

use crate::theme::{self, Typo};

/// Logo de Tidy : une boîte à barre de titre lime qui contient quatre icônes
/// violettes. Même dessin que l'icône de l'app (`tools/gen_icon.py`).
pub fn logo(ui: &mut Ui, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
    paint_logo(ui.painter(), rect);
    response
}

/// Dessin du logo sur une grille de 48 (voir `tools/gen_icon.py`).
pub fn paint_logo(painter: &egui::Painter, rect: egui::Rect) {
    let u = rect.width() / 48.0;
    let at = |x: f32, y: f32, w: f32, h: f32| {
        egui::Rect::from_min_size(
            rect.min + egui::vec2(x * u, y * u),
            egui::vec2(w * u, h * u),
        )
    };
    let r = |v: f32| (v * u).round().clamp(0.0, 255.0) as u8;
    // Corps de la boîte.
    painter.rect_filled(at(0.0, 0.0, 48.0, 48.0), r(10.0), theme::BORDER);
    // Barre de titre lime (coins du haut arrondis).
    painter.rect_filled(
        at(0.0, 0.0, 48.0, 12.0),
        CornerRadius {
            nw: r(10.0),
            ne: r(10.0),
            sw: 0,
            se: 0,
        },
        theme::ACCENT_2,
    );
    // Quatre icônes violettes.
    for (x, y) in [(8.0, 18.0), (28.0, 18.0), (8.0, 34.0), (28.0, 34.0)] {
        painter.rect_filled(at(x, y, 12.0, 10.0), r(3.0), theme::ACCENT);
    }
}

/// En-tête des fenêtres : logo et nom de l'app en Unbounded, comme Tappy.
pub fn app_header(ui: &mut Ui, title: &str) {
    ui.horizontal(|ui| {
        logo(ui, 30.0);
        ui.add_space(4.0);
        ui.label(RichText::new(title).title().size(24.0));
    });
}

/// Texte secondaire (gris, petit).
pub fn dim(text: impl Into<String>) -> RichText {
    RichText::new(text)
        .color(theme::TEXT_DIM)
        .size(theme::SIZE_S)
}

/// Ligne d'aide grise, qui passe à la ligne si besoin.
pub fn hint(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.add(Label::new(dim(text)).wrap())
}

/// Titre de page (Unbounded).
pub fn page_title(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(RichText::new(text).title().size(20.0))
}

/// Titre de section.
pub fn section_title(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(RichText::new(text).bold().size(theme::SIZE_L))
}

/// Carte : surface arrondie avec bordure, pour regrouper des réglages. Occupe toute
/// la largeur disponible, marges et bordure comprises.
pub fn card<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    const MARGIN: i8 = 16;
    let inner = ui.available_width() - 2.0 * (f32::from(MARGIN) + 1.0);
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(theme::CARD_RADIUS))
        .inner_margin(MARGIN)
        .show(ui, |ui| {
            ui.set_width(inner.max(0.0));
            add_contents(ui)
        })
        .inner
}

/// Bouton plein qui garde un retour visuel au survol et au clic.
pub fn filled(ui: &mut Ui, fill: Color32, button: Button<'_>) -> Response {
    ui.scope(|ui| {
        let hover = fill.lerp_to_gamma(Color32::WHITE, 0.15);
        let press = fill.lerp_to_gamma(Color32::BLACK, 0.15);
        let w = &mut ui.visuals_mut().widgets;
        for (state, color) in [
            (&mut w.inactive, fill),
            (&mut w.hovered, hover),
            (&mut w.active, press),
        ] {
            state.weak_bg_fill = color;
            state.bg_fill = color;
            state.bg_stroke = Stroke::new(1.0, color);
        }
        ui.add(button)
    })
    .inner
}

/// Bouton principal (lime, texte noir).
pub fn primary(ui: &mut Ui, text: impl Into<String>) -> Response {
    let button = Button::new(RichText::new(text).bold().color(theme::ON_ACCENT_2));
    filled(ui, theme::ACCENT_2, button)
}

/// Bouton « danger » (rouge).
pub fn danger(ui: &mut Ui, text: impl Into<String>) -> Response {
    let button = Button::new(RichText::new(text).bold().color(theme::on(theme::DANGER)));
    filled(ui, theme::DANGER, button)
}

/// Choix sélectionnable (onglet, option). Sélectionné : fond lime, texte noir lisible.
pub fn choice(ui: &mut Ui, selected: bool, text: impl Into<RichText>) -> Response {
    let mut text: RichText = text.into();
    if selected {
        text = text.color(theme::ON_ACCENT_2);
    }
    ui.selectable_label(selected, text)
}

/// [`choice`] qui affecte `value` à `current` au clic.
pub fn choice_value<V: PartialEq>(
    ui: &mut Ui,
    current: &mut V,
    value: V,
    text: impl Into<RichText>,
) -> Response {
    let selected = *current == value;
    let mut response = choice(ui, selected, text);
    if response.clicked() && !selected {
        *current = value;
        response.mark_changed();
    }
    response
}

/// Navigation latérale : une entrée par ligne, icône et libellé alignés à gauche,
/// pleine largeur. Renvoie l'indice cliqué.
pub fn nav<'a>(
    ui: &mut Ui,
    selected: usize,
    items: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<usize> {
    let mut clicked = None;
    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
        ui.spacing_mut().button_padding = egui::vec2(12.0, 8.0);
        ui.spacing_mut().item_spacing.y = 4.0;
        for (i, (icon, label)) in items.into_iter().enumerate() {
            let text = RichText::new(format!("{icon}   {label}")).size(theme::SIZE_M);
            if choice(ui, i == selected, text).clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

/// Pastille de couleur cliquable.
pub fn swatch(ui: &mut Ui, color: Color32, selected: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, 6, color);
    if selected {
        painter.rect_stroke(rect, 6, Stroke::new(2.0, theme::TEXT), StrokeKind::Outside);
    } else if response.hovered() {
        painter.rect_stroke(
            rect,
            6,
            Stroke::new(1.0, theme::TEXT_DIM),
            StrokeKind::Outside,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Badge d'état (fond plein arrondi), comme « Pad simulé » dans Tappy.
pub fn badge(ui: &mut Ui, fill: Color32, text: impl Into<String>) -> Response {
    egui::Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(255))
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .bold()
                    .size(theme::SIZE_S)
                    .color(theme::on(fill)),
            )
        })
        .response
}

/// Modale de confirmation. `Some(true)` : confirmé (bouton ou Entrée), `Some(false)` :
/// annulé (bouton, Échap ou clic à côté), `None` : en attente.
pub fn confirm(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    body: &str,
    confirm_label: &str,
    destructive: bool,
) -> Option<bool> {
    let mut answer = None;
    let enter = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
    let id = Id::unique(id);
    let modal = egui::Modal::new(id)
        .area(egui::Modal::default_area(id).anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO))
        .frame(egui::Frame::popup(&ctx.global_style()).inner_margin(16))
        .show(ctx, |ui| {
            ui.set_width(380.0);
            ui.add(Label::new(RichText::new(title).bold().size(theme::SIZE_L)).wrap());
            if !body.is_empty() {
                ui.add_space(4.0);
                ui.add(Label::new(RichText::new(body).color(theme::TEXT_DIM)).wrap());
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                let clicked = if destructive {
                    danger(ui, confirm_label).clicked()
                } else {
                    primary(ui, confirm_label).clicked()
                };
                if clicked {
                    answer = Some(true);
                }
                if ui.button(crate::t!("common.cancel")).clicked() {
                    answer = Some(false);
                }
            });
        });
    if answer.is_none() && enter {
        answer = Some(true);
    }
    if answer.is_none() && modal.should_close() {
        answer = Some(false);
    }
    answer
}

/// Coupe un texte à `max` caractères (« … » compris).
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncate_adds_an_ellipsis() {
        assert_eq!(truncate("Base", 10), "Base");
        assert_eq!(truncate("Montage vidéo", 6), "Monta…");
    }
}
