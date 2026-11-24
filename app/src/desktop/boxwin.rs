//! Dessin et interactions d'une fenêtre de boîte.
//!
//! La fenêtre a la taille de la boîte : la carte occupe toute la fenêtre. Le système
//! déplace et redimensionne la fenêtre lui-même (`StartDrag`, `BeginResize`), ce qui
//! reste fluide même quand l'interface ne se redessine pas.

use egui::viewport::ResizeDirection;
use egui::{Align2, CornerRadius, CursorIcon, Sense, Stroke, StrokeKind, ViewportCommand, vec2};
use tidy_core::model::DeskBox;
use tidy_ui::{icons, t, theme};

/// Hauteur de la barre de titre.
pub const TITLE_HEIGHT: f32 = 30.0;
/// Largeur de la zone de redimensionnement le long des bords.
const EDGE: f32 = 6.0;
/// Taille minimale d'une boîte.
pub const MIN_SIZE: [f32; 2] = [160.0, 90.0];

/// Ce que l'utilisateur a demandé dans la boîte.
#[derive(Debug, Default, PartialEq)]
pub struct Output {
    pub new_box: bool,
    pub delete: bool,
}

/// Bord ou coin sous `p`, s'il y en a un.
fn edge_at(rect: egui::Rect, p: egui::Pos2) -> Option<ResizeDirection> {
    let left = p.x < rect.left() + EDGE;
    let right = p.x > rect.right() - EDGE;
    let top = p.y < rect.top() + EDGE;
    let bottom = p.y > rect.bottom() - EDGE;
    Some(match (left, right, top, bottom) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (_, true, true, _) => ResizeDirection::NorthEast,
        (true, _, _, true) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, ..) => ResizeDirection::West,
        (_, true, ..) => ResizeDirection::East,
        (_, _, true, _) => ResizeDirection::North,
        (_, _, _, true) => ResizeDirection::South,
        _ => return None,
    })
}

fn cursor_for(direction: ResizeDirection) -> CursorIcon {
    match direction {
        ResizeDirection::North | ResizeDirection::South => CursorIcon::ResizeVertical,
        ResizeDirection::East | ResizeDirection::West => CursorIcon::ResizeHorizontal,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorIcon::ResizeNwSe,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::ResizeNeSw,
    }
}

pub fn draw(ui: &mut egui::Ui, b: &DeskBox, demo: bool) -> Output {
    let mut out = Output::default();
    let ctx = ui.ctx().clone();
    let rect = ui.max_rect();
    let painter = ui.painter().clone();
    let radius = CornerRadius::same(theme::CARD_RADIUS);
    painter.rect_filled(rect, radius, theme::SURFACE.gamma_multiply(0.94));
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );

    // Barre de titre : déplacer la fenêtre, menu au clic droit.
    let title_rect = egui::Rect::from_min_size(rect.min, vec2(rect.width(), TITLE_HEIGHT));
    let title = ui.interact(
        title_rect.shrink(EDGE / 2.0),
        ui.scope_id().with("titre"),
        Sense::click_and_drag(),
    );
    if title.hovered() || title.dragged() {
        painter.rect_filled(
            title_rect.shrink(1.0),
            CornerRadius {
                nw: theme::CARD_RADIUS - 1,
                ne: theme::CARD_RADIUS - 1,
                sw: 0,
                se: 0,
            },
            theme::SURFACE_HI,
        );
    }
    painter.text(
        title_rect.left_center() + vec2(12.0, 0.0),
        Align2::LEFT_CENTER,
        format!("{}  {}", icons::MENU, b.name),
        theme::strong(theme::SIZE_M),
        theme::TEXT,
    );
    painter.line_segment(
        [title_rect.left_bottom(), title_rect.right_bottom()],
        Stroke::new(1.0, theme::BORDER),
    );
    if title.drag_started() {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    title.context_menu(|ui| {
        if ui
            .button(format!("{}  {}", icons::PLUS, t!("desktop.menu_new_box")))
            .clicked()
        {
            out.new_box = true;
            ui.close();
        }
        if ui
            .button(format!(
                "{}  {}",
                icons::DELETE,
                t!("desktop.menu_delete_box")
            ))
            .clicked()
        {
            out.delete = true;
            ui.close();
        }
    });

    // Contenu.
    let body = egui::Rect::from_min_max(title_rect.left_bottom(), rect.max).shrink(16.0);
    if demo {
        for (i, color) in [theme::ACCENT, theme::ACCENT_2, theme::ACCENT, theme::ACCENT]
            .into_iter()
            .enumerate()
        {
            let at = body.min + vec2(i as f32 * 64.0, 4.0);
            let icon = egui::Rect::from_min_size(at, vec2(44.0, 44.0));
            if icon.right() > body.right() {
                break;
            }
            painter.rect_filled(icon, CornerRadius::same(10), color.gamma_multiply(0.85));
            painter.rect_filled(
                egui::Rect::from_min_size(at + vec2(4.0, 56.0), vec2(36.0, 6.0)),
                CornerRadius::same(3),
                theme::BORDER,
            );
        }
    }
    let hint = if demo {
        t!("desktop.demo_hint")
    } else {
        t!("desktop.empty_box")
    };
    let galley = painter.layout(
        hint.to_owned(),
        egui::FontId::proportional(theme::SIZE_S),
        theme::TEXT_DIM,
        body.width(),
    );
    let at = if demo {
        body.left_bottom() - vec2(0.0, galley.size().y)
    } else {
        body.center() - galley.size() / 2.0
    };
    painter
        .with_clip_rect(body)
        .galley(at, galley, theme::TEXT_DIM);

    // Bords : redimensionner la fenêtre.
    if let Some(pointer) = ctx.pointer_hover_pos()
        && let Some(direction) = edge_at(rect, pointer)
    {
        ctx.set_cursor_icon(cursor_for(direction));
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
        }
    } else if title.dragged() {
        ctx.set_cursor_icon(CursorIcon::Grabbing);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_and_corners() {
        let r = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(300.0, 200.0));
        assert_eq!(edge_at(r, egui::pos2(150.0, 100.0)), None);
        assert_eq!(
            edge_at(r, egui::pos2(2.0, 100.0)),
            Some(ResizeDirection::West)
        );
        assert_eq!(
            edge_at(r, egui::pos2(298.0, 198.0)),
            Some(ResizeDirection::SouthEast)
        );
        assert_eq!(
            edge_at(r, egui::pos2(150.0, 1.0)),
            Some(ResizeDirection::North)
        );
    }
}
