//! Éditeur d'apparence d'une boîte (couleurs, opacité, titre, coins, verre, taille des
//! icônes), avec un aperçu. Sert à l'apparence par défaut et à celle de chaque boîte.

use egui::{Color32, CornerRadius, Stroke, StrokeKind};
use tidy_core::model::{Appearance, Rgba, TitleVisibility};
use tidy_ui::{icons, t, theme, widgets};

/// Couleur egui d'une couleur du modèle.
pub fn color(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c.0, c.1, c.2, c.3)
}

/// Couleur du modèle d'une couleur egui.
pub fn rgba(c: Color32) -> Rgba {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Rgba(r, g, b, a)
}

/// Pastilles pour une couleur du modèle. Renvoie `true` si elle a changé.
fn color_row(ui: &mut egui::Ui, value: &mut Rgba) -> bool {
    let mut c = color(*value);
    let changed = widgets::swatches(ui, &mut c);
    if changed {
        *value = rgba(c);
    }
    changed
}

/// Ce qui a changé dans l'éditeur.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Changed {
    /// Un clic (pastille, choix, interrupteur) : à enregistrer tout de suite.
    pub now: bool,
    /// Un curseur : à enregistrer quand la souris est relâchée.
    pub dragging: bool,
}

impl Changed {
    pub fn any(self) -> bool {
        self.now || self.dragging
    }
}

/// Éditeur complet, aperçu compris. `title` : nom affiché dans l'aperçu.
pub fn editor(ui: &mut egui::Ui, a: &mut Appearance, title: &str) -> Changed {
    let mut changed = Changed::default();
    ui.horizontal_top(|ui| {
        preview(ui, a, title);
        ui.add_space(16.0);
        ui.vertical(|ui| {
            ui.label(widgets::dim(t!("boxes_page.background")));
            changed.now |= color_row(ui, &mut a.background);
            ui.add_space(4.0);
            ui.label(widgets::dim(t!("boxes_page.title_background")));
            changed.now |= color_row(ui, &mut a.title_background);
            ui.add_space(4.0);
            ui.label(widgets::dim(t!("boxes_page.title_color")));
            changed.now |= color_row(ui, &mut a.title_color);
        });
    });
    widgets::row_separator(ui);

    let mut percent = (a.opacity * 100.0).round();
    if widgets::setting_row(ui, icons::OPACITY, t!("boxes_page.opacity"), None, |ui| {
        ui.add(egui::Slider::new(&mut percent, 10.0..=100.0).suffix(" %"))
            .changed()
    }) {
        // L'opacité « souris ailleurs » suit, sauf si elle a été réglée à part.
        let same = (a.opacity_away - a.opacity).abs() < 0.005;
        a.opacity = percent / 100.0;
        if same {
            a.opacity_away = a.opacity;
        }
        changed.dragging = true;
    }
    widgets::row_separator(ui);

    widgets::setting_row(
        ui,
        icons::TITLE,
        t!("boxes_page.title_visibility"),
        None,
        |ui| {
            // De droite à gauche : on ajoute les choix à l'envers.
            for (value, label) in [
                (TitleVisibility::Never, t!("boxes_page.title_never")),
                (TitleVisibility::OnHover, t!("boxes_page.title_on_hover")),
                (TitleVisibility::Always, t!("boxes_page.title_always")),
            ] {
                changed.now |=
                    widgets::choice_value(ui, &mut a.title_visibility, value, label).changed();
            }
        },
    );
    widgets::row_separator(ui);

    if widgets::setting_row(ui, icons::CORNERS, t!("boxes_page.corners"), None, |ui| {
        ui.add(
            egui::Slider::new(&mut a.corner_radius, 0.0..=24.0)
                .suffix(" pt")
                .fixed_decimals(0),
        )
        .changed()
    }) {
        changed.dragging = true;
    }
    widgets::row_separator(ui);

    if widgets::setting_row(
        ui,
        icons::ICON_SIZE,
        t!("boxes_page.icon_size"),
        None,
        |ui| {
            ui.add(
                egui::Slider::new(&mut a.icon_size, 24.0..=96.0)
                    .suffix(" pt")
                    .fixed_decimals(0),
            )
            .changed()
        },
    ) {
        changed.dragging = true;
    }
    widgets::row_separator(ui);

    changed.now |= widgets::setting_row(
        ui,
        icons::GLASS,
        t!("boxes_page.glass"),
        Some(t!("boxes_page.glass_hint")),
        |ui| widgets::toggle(ui, &mut a.glass).changed(),
    );
    widgets::row_separator(ui);
    changed.now |= widgets::setting_row(ui, icons::BOX, t!("boxes_page.border"), None, |ui| {
        widgets::toggle(ui, &mut a.border).changed()
    });
    changed
}

/// Aperçu d'une boîte avec cette apparence, sur un « bureau » violet sombre.
pub fn preview(ui: &mut egui::Ui, a: &Appearance, title: &str) {
    let size = egui::vec2(210.0, 150.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    // Fond de bureau.
    painter.rect_filled(
        rect,
        CornerRadius::same(12),
        theme::ACCENT.gamma_multiply(0.35),
    );
    let desk = rect.shrink(16.0);
    let radius = CornerRadius::same(a.corner_radius.clamp(0.0, 40.0).round() as u8);
    let mut bg = color(a.background).gamma_multiply(a.opacity.clamp(0.0, 1.0));
    if a.glass {
        bg = bg.lerp_to_gamma(Color32::from_white_alpha(bg.a()), 0.12);
    }
    painter.rect_filled(desk, radius, bg);
    let title_h = 22.0;
    let show_title = a.title_visibility == TitleVisibility::Always;
    let mut body = desk;
    if show_title {
        let bar = egui::Rect::from_min_size(desk.min, egui::vec2(desk.width(), title_h));
        let bar_radius = CornerRadius {
            nw: radius.nw,
            ne: radius.ne,
            sw: 0,
            se: 0,
        };
        painter.rect_filled(bar, bar_radius, color(a.title_background));
        painter.text(
            bar.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            widgets::truncate(title, 22),
            theme::strong(12.0),
            color(a.title_color),
        );
        body.min.y += title_h;
    }
    // Icônes : leur taille suit le réglage (48 pt → 22 px dans l'aperçu).
    let icon = (a.icon_size / 48.0 * 22.0).clamp(10.0, 40.0);
    let gap = 8.0;
    let mut x = body.left() + gap;
    let mut y = body.top() + gap;
    for _ in 0..8 {
        if x + icon > body.right() - gap / 2.0 {
            x = body.left() + gap;
            y += icon + gap;
        }
        if y + icon > body.bottom() - gap / 2.0 {
            break;
        }
        let r = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(icon, icon * 0.85));
        painter.rect_filled(
            r,
            CornerRadius::same(3),
            theme::ACCENT_2.gamma_multiply(0.9),
        );
        x += icon + gap;
    }
    if a.border {
        painter.rect_stroke(
            desk,
            radius,
            Stroke::new(1.0, theme::TEXT.gamma_multiply(0.25)),
            StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_round_trip() {
        let c = Rgba(0x12, 0x34, 0x56, 255);
        assert_eq!(rgba(color(c)), c);
        // Translucide : egui prémultiplie, à une unité près.
        let back = rgba(color(Rgba(0x80, 0x40, 0x20, 200)));
        assert_eq!(back.3, 200);
        assert!(back.0.abs_diff(0x80) <= 1 && back.1.abs_diff(0x40) <= 1);
    }
}
