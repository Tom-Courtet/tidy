//! Dessin d'une surface du bureau (un écran) et de la boîte de démonstration.

use egui::{Align2, CornerRadius, Pos2, Sense, Stroke, StrokeKind, Vec2, pos2, vec2};
use tidy_core::geometry::{Point, Rect};
use tidy_core::screen::Screen;
use tidy_ui::{icons, t, theme};

/// Hauteur de la barre de titre d'une boîte.
pub const TITLE_HEIGHT: f32 = 30.0;

/// Titre (unique) de la fenêtre de l'écran `i` : sert à retrouver les fenêtres
/// secondaires côté système.
pub fn title(i: usize) -> String {
    format!("Tidy · bureau {}", i + 1)
}

/// Fenêtre transparente, sans bordure, à la taille de l'écran. Le niveau, les Spaces et
/// le cadre exact sont réglés ensuite par `tidy-platform`.
pub fn builder(screen: &Screen, title: &str) -> egui::ViewportBuilder {
    let f = screen.frame;
    egui::ViewportBuilder::default()
        .with_title(title)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_has_shadow(false)
        .with_taskbar(false)
        .with_position([f.x, f.y])
        .with_inner_size([f.width, f.height])
}

/// Ce qu'il faut dessiner sur une surface.
pub struct Frame {
    /// Origine de l'écran, en coordonnées globales.
    pub origin: Point,
    /// Boîte de démonstration, en coordonnées globales.
    pub demo: Rect,
    pub hidden: bool,
    pub managed: bool,
    pub title: String,
}

/// Ce que l'utilisateur a fait sur la surface.
#[derive(Default)]
pub struct Output {
    /// Déplacement de la boîte pendant cette image.
    pub dragged: Option<Vec2>,
    pub drag_stopped: bool,
    /// Double-clic sur une zone vide (mode géré).
    pub background_double_clicked: bool,
}

fn to_local(r: Rect, origin: Point) -> egui::Rect {
    egui::Rect::from_min_size(
        pos2(r.x - origin.x, r.y - origin.y),
        vec2(r.width, r.height),
    )
}

pub fn draw(ui: &mut egui::Ui, frame: Frame) -> Output {
    let mut out = Output::default();
    let full = ui.max_rect();
    // Fond : seul le mode géré l'écoute (double-clic pour masquer).
    if frame.managed {
        let bg = ui.interact(full, ui.scope_id().with("fond"), Sense::click());
        if bg.double_clicked() {
            out.background_double_clicked = true;
        }
    }
    if frame.hidden {
        return out;
    }
    let rect = to_local(frame.demo, frame.origin);
    if !rect.intersects(full) {
        return out;
    }
    let painter = ui.painter();
    let radius = CornerRadius::same(theme::CARD_RADIUS);
    painter.rect_filled(rect, radius, theme::SURFACE.gamma_multiply(0.92));
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );

    // Barre de titre : poignée de déplacement.
    let title_rect = egui::Rect::from_min_size(rect.min, vec2(rect.width(), TITLE_HEIGHT));
    let title_response = ui.interact(title_rect, ui.scope_id().with("titre-démo"), Sense::drag());
    let title_fill = if title_response.hovered() || title_response.dragged() {
        theme::SURFACE_HI
    } else {
        theme::SURFACE
    };
    painter.rect_filled(
        title_rect.shrink(1.0),
        CornerRadius {
            nw: theme::CARD_RADIUS - 1,
            ne: theme::CARD_RADIUS - 1,
            sw: 0,
            se: 0,
        },
        title_fill,
    );
    painter.text(
        title_rect.left_center() + vec2(12.0, 0.0),
        Align2::LEFT_CENTER,
        format!("{}  {}", icons::MENU, frame.title),
        tidy_ui::theme::strong(theme::SIZE_M),
        theme::TEXT,
    );
    painter.text(
        title_rect.right_center() - vec2(12.0, 0.0),
        Align2::RIGHT_CENTER,
        icons::ROLL_UP,
        egui::FontId::proportional(theme::SIZE_M),
        theme::TEXT_DIM,
    );
    painter.line_segment(
        [title_rect.left_bottom(), title_rect.right_bottom()],
        Stroke::new(1.0, theme::BORDER),
    );

    // Contenu : quatre fausses icônes et l'aide.
    let body = egui::Rect::from_min_max(title_rect.left_bottom(), rect.max).shrink(16.0);
    for (i, color) in [theme::ACCENT, theme::ACCENT_2, theme::ACCENT, theme::ACCENT]
        .into_iter()
        .enumerate()
    {
        let at = body.min + vec2(i as f32 * 64.0, 4.0);
        let icon = egui::Rect::from_min_size(at, vec2(44.0, 44.0));
        painter.rect_filled(icon, CornerRadius::same(10), color.gamma_multiply(0.85));
        painter.rect_filled(
            egui::Rect::from_min_size(at + vec2(4.0, 56.0), vec2(36.0, 6.0)),
            CornerRadius::same(3),
            theme::BORDER,
        );
    }
    painter.text(
        Pos2::new(body.min.x, body.max.y),
        Align2::LEFT_BOTTOM,
        t!("desktop.demo_hint"),
        egui::FontId::proportional(theme::SIZE_S),
        theme::TEXT_DIM,
    );

    if title_response.dragged() {
        out.dragged = Some(title_response.drag_delta());
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if title_response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if title_response.drag_stopped() {
        out.drag_stopped = true;
    }
    out
}
