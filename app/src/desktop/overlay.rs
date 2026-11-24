//! Petites fenêtres flottantes du bureau, ouvertes le temps qu'il faut puis fermées
//! (leur mémoire est rendue) :
//! - la fenêtre des menus et des confirmations (une boîte est trop petite pour les
//!   contenir) ;
//! - le bandeau « N icônes non affectées · Appliquer les règles » ;
//! - le bouton « Créer une boîte » proposé après un rectangle tracé sur le bureau.

use std::time::{Duration, Instant};

use egui::{
    Align2, CornerRadius, FontId, Key, Modifiers, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2,
    ViewportBuilder, ViewportCommand, ViewportId, WindowLevel, pos2, vec2,
};
use tidy_core::geometry;
use tidy_core::model::BoxId;
use tidy_core::screen::Screen;
use tidy_ui::{icons, t, theme, widgets};

use super::action::Action;
use super::menu::{self, Entry, EntryKind, Page};

/// Durée de l'offre « Créer une boîte » après un rectangle tracé.
const DRAW_OFFER: Duration = Duration::from_secs(8);
/// Durée d'un message du bandeau.
const PILL_MESSAGE: Duration = Duration::from_secs(6);
/// Largeur d'une confirmation.
const CONFIRM_WIDTH: f32 = 340.0;

/// Contenu de la fenêtre flottante.
#[derive(Debug, Clone, PartialEq)]
pub enum PopupKind {
    /// Menu ☰ d'une boîte, à une page donnée.
    BoxMenu { b: BoxId, page: Page },
    /// Menu d'éléments (repli du menu natif), déjà construit.
    Menu { entries: Vec<Entry> },
    /// Confirmation : `action` est appliquée si l'utilisateur confirme.
    Confirm {
        title: String,
        body: String,
        confirm: String,
        danger: bool,
        action: Action,
    },
}

/// Fenêtre flottante ouverte.
pub struct Popup {
    pub kind: PopupKind,
    /// Coin haut gauche voulu (coordonnées globales, avant de rester dans l'écran).
    anchor: Pos2,
    size: Vec2,
    serial: u64,
    focused_once: bool,
}

/// Offre « Créer une boîte » après un rectangle tracé sur le bureau.
struct DrawOffer {
    rect: geometry::Rect,
    since: Instant,
}

#[derive(Default)]
pub struct Overlay {
    popup: Option<Popup>,
    serial: u64,
    draw: Option<DrawOffer>,
    /// Nombre d'icônes non affectées au moment où le bandeau a été fermé.
    dismissed: Option<usize>,
    pill_placed: bool,
    pill_message: Option<(String, Instant)>,
}

/// Cadre de la fenêtre : `size` placé à `anchor`, gardé dans le cadre utile de son
/// écran (retourné vers le haut s'il dépasse en bas, vers la gauche à droite).
pub fn place(anchor: Pos2, size: Vec2, screen: geometry::Rect) -> Rect {
    let mut x = anchor.x;
    let mut y = anchor.y;
    if x + size.x > screen.right() {
        x = (anchor.x - size.x).max(screen.x);
    }
    if y + size.y > screen.bottom() {
        y = (anchor.y - size.y).max(screen.y);
    }
    x = x.clamp(screen.x, (screen.right() - size.x).max(screen.x));
    y = y.clamp(screen.y, (screen.bottom() - size.y).max(screen.y));
    Rect::from_min_size(pos2(x, y), size)
}

fn screen_of(screens: &[Screen], p: Pos2) -> geometry::Rect {
    super::screen_at(screens, geometry::Point { x: p.x, y: p.y }).visible_frame
}

impl Overlay {
    /// Ouvre une fenêtre flottante à `anchor` (coordonnées globales).
    pub fn open(&mut self, kind: PopupKind, anchor: Pos2) {
        self.serial += 1;
        self.popup = Some(Popup {
            kind,
            anchor,
            size: vec2(menu::WIDTH, 120.0),
            serial: self.serial,
            focused_once: false,
        });
    }

    pub fn close_popup(&mut self) {
        self.popup = None;
    }

    pub fn popup_kind(&self) -> Option<&PopupKind> {
        self.popup.as_ref().map(|p| &p.kind)
    }

    /// Une fenêtre flottante vise cette boîte (elle ne doit pas s'enrouler).
    pub fn busy_with(&self, b: BoxId) -> bool {
        match self.popup_kind() {
            Some(PopupKind::BoxMenu { b: id, .. }) => *id == b,
            Some(_) => true,
            None => false,
        }
    }

    /// Propose « Créer une boîte » pour ce rectangle.
    pub fn offer_draw(&mut self, rect: geometry::Rect) {
        self.draw = Some(DrawOffer {
            rect,
            since: Instant::now(),
        });
    }

    /// Message affiché dans le bandeau (résultat de « Appliquer les règles »).
    pub fn pill_message(&mut self, text: String) {
        self.pill_message = Some((text, Instant::now()));
    }

    pub fn dismiss_unassigned(&mut self, count: usize) {
        self.dismissed = Some(count);
    }

    /// Dessine la fenêtre flottante. `box_menu` construit le menu d'une boîte.
    pub fn show_popup(
        &mut self,
        ctx: &egui::Context,
        screens: &[Screen],
        box_menu: impl Fn(BoxId, Page) -> Vec<Entry>,
        out: &mut Vec<Action>,
    ) {
        let Some(popup) = self.popup.as_mut() else {
            return;
        };
        // Taille voulue.
        let entries = match &popup.kind {
            PopupKind::BoxMenu { b, page } => Some(box_menu(*b, *page)),
            PopupKind::Menu { entries } => Some(entries.clone()),
            PopupKind::Confirm { .. } => None,
        };
        if let Some(entries) = &entries {
            popup.size = vec2(menu::WIDTH, menu::height(entries));
        } else {
            popup.size.x = CONFIRM_WIDTH;
        }
        let frame = place(popup.anchor, popup.size, screen_of(screens, popup.anchor));
        let builder = ViewportBuilder::default()
            .with_title("Tidy · menu")
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_has_shadow(true)
            .with_taskbar(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_active(true)
            .with_position(frame.min)
            .with_inner_size(frame.size());
        let id = ViewportId::from_hash_of(("tidy-popup", popup.serial));
        let mut close = false;
        let mut measured = None;
        ctx.show_viewport_immediate(id, builder, |ui, _| {
            let ctx = ui.ctx().clone();
            let rect = ui.max_rect();
            card(ui.painter(), rect);
            let inner = rect.shrink(menu::PADDING);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
            match &mut popup.kind {
                PopupKind::BoxMenu { page, .. } => {
                    if let Some(kind) = menu::draw(&mut child, entries.as_deref().unwrap_or(&[])) {
                        match kind {
                            EntryKind::Open(p) => *page = p,
                            EntryKind::Do(action) => {
                                out.push(action);
                                close = true;
                            }
                        }
                    }
                }
                PopupKind::Menu { .. } => {
                    if let Some(EntryKind::Do(action)) =
                        menu::draw(&mut child, entries.as_deref().unwrap_or(&[]))
                    {
                        out.push(action);
                        close = true;
                    }
                }
                PopupKind::Confirm {
                    title,
                    body,
                    confirm,
                    danger,
                    action,
                } => {
                    let answer = confirm_ui(&mut child, title, body, confirm, *danger);
                    measured = Some(child.min_rect().height() + 2.0 * menu::PADDING + 8.0);
                    match answer {
                        Some(true) => {
                            out.push(action.clone());
                            close = true;
                        }
                        Some(false) => close = true,
                        None => {}
                    }
                }
            }
            // Fermer : Échap, ou la fenêtre perd le focus après l'avoir eu.
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                close = true;
            }
            match ctx.input(|i| i.viewport().focused) {
                Some(true) => popup.focused_once = true,
                // Un clic ailleurs (autre appli, bureau) : refermer. Sans focus du
                // tout, un clic dans une boîte la referme (voir `Desktop`).
                Some(false) if popup.focused_once => close = true,
                _ => {}
            }
        });
        if let Some(h) = measured
            && (h - popup.size.y).abs() > 1.0
        {
            popup.size.y = h;
            let frame = place(popup.anchor, popup.size, screen_of(screens, popup.anchor));
            ctx.send_viewport_cmd_to(id, ViewportCommand::InnerSize(frame.size()));
            ctx.send_viewport_cmd_to(id, ViewportCommand::OuterPosition(frame.min));
        }
        if close {
            self.popup = None;
            ctx.request_repaint();
        }
    }

    /// Bandeau des icônes non affectées (`count` > 0), en bas de l'écran principal.
    pub fn show_unassigned(
        &mut self,
        ctx: &egui::Context,
        screens: &[Screen],
        count: usize,
        hidden: bool,
        out: &mut Vec<Action>,
    ) {
        if self.dismissed.is_some_and(|n| n != count) {
            self.dismissed = None;
        }
        if self
            .pill_message
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed() > PILL_MESSAGE)
        {
            self.pill_message = None;
        }
        if count == 0 || hidden || self.dismissed.is_some() {
            self.pill_placed = false;
            return;
        }
        let Some(primary) = screens.iter().find(|s| s.primary).or(screens.first()) else {
            return;
        };
        let v = primary.visible_frame;
        let size = vec2(430.0, 44.0);
        let pos = pos2(v.x + (v.width - size.x) / 2.0, v.bottom() - size.y - 24.0);
        const TITLE: &str = "Tidy · non affectées";
        let builder = ViewportBuilder::default()
            .with_title(TITLE)
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_has_shadow(false)
            .with_taskbar(false)
            .with_active(false)
            .with_position(pos)
            .with_inner_size(size);
        let message = self.pill_message.as_ref().map(|(m, _)| m.clone());
        if message.is_some() {
            ctx.request_repaint_after_for(PILL_MESSAGE, ViewportId::ROOT);
        }
        ctx.show_viewport_immediate(
            ViewportId::from_hash_of("tidy-unassigned"),
            builder,
            |ui, _| {
                let rect = ui.max_rect();
                let painter = ui.painter().clone();
                let radius = CornerRadius::same((rect.height() / 2.0) as u8);
                painter.rect_filled(rect, radius, theme::SURFACE.gamma_multiply(0.96));
                painter.rect_stroke(
                    rect,
                    radius,
                    Stroke::new(1.0, theme::BORDER),
                    StrokeKind::Inside,
                );
                painter.text(
                    rect.left_center() + vec2(18.0, 0.0),
                    Align2::LEFT_CENTER,
                    icons::AUTO_TIDY,
                    FontId::proportional(theme::SIZE_L),
                    theme::ACCENT_2,
                );
                let text = message
                    .clone()
                    .unwrap_or_else(|| t!("box.unassigned", n = count));
                painter.text(
                    rect.left_center() + vec2(42.0, 0.0),
                    Align2::LEFT_CENTER,
                    widgets::truncate(&text, 34),
                    FontId::proportional(theme::SIZE_M),
                    theme::TEXT,
                );
                // Fermer.
                let close = Rect::from_center_size(
                    rect.right_center() - vec2(22.0, 0.0),
                    Vec2::splat(26.0),
                );
                let r = ui.interact(close, ui.scope_id().with("fermer"), Sense::click());
                if r.hovered() {
                    painter.rect_filled(close, CornerRadius::same(13), theme::SURFACE_HI);
                }
                painter.text(
                    close.center(),
                    Align2::CENTER_CENTER,
                    icons::CLOSE,
                    FontId::proportional(theme::SIZE_M),
                    theme::TEXT_DIM,
                );
                if r.clicked() {
                    out.push(Action::DismissUnassigned);
                }
                // Appliquer les règles.
                if message.is_none() {
                    let label = t!("box.apply_rules");
                    let w = 150.0;
                    let button = Rect::from_min_size(
                        pos2(close.left() - 8.0 - w, rect.top() + 7.0),
                        vec2(w, rect.height() - 14.0),
                    );
                    let r = ui.interact(button, ui.scope_id().with("appliquer"), Sense::click());
                    let fill = if r.hovered() {
                        theme::ACCENT_2.gamma_multiply(0.85)
                    } else {
                        theme::ACCENT_2
                    };
                    painter.rect_filled(button, CornerRadius::same(15), fill);
                    painter.text(
                        button.center(),
                        Align2::CENTER_CENTER,
                        label,
                        theme::strong(theme::SIZE_S),
                        theme::ON_ACCENT_2,
                    );
                    if r.clicked() {
                        out.push(Action::ApplyRules);
                    }
                }
            },
        );
        if !self.pill_placed {
            self.pill_placed = tidy_platform::current().make_desktop_surface_titled(
                TITLE,
                geometry::Rect::new(pos.x, pos.y, size.x, size.y),
                tidy_platform::SurfaceLevel::DesktopIcons,
            );
            ctx.request_repaint();
        }
    }

    /// Bouton « Créer une boîte » à côté du rectangle tracé.
    pub fn show_draw_offer(
        &mut self,
        ctx: &egui::Context,
        screens: &[Screen],
        out: &mut Vec<Action>,
    ) {
        let Some(offer) = &self.draw else {
            return;
        };
        let left = DRAW_OFFER.saturating_sub(offer.since.elapsed());
        if left.is_zero() {
            self.draw = None;
            return;
        }
        ctx.request_repaint_after_for(left, ViewportId::ROOT);
        let r = offer.rect;
        let size = vec2(190.0, 40.0);
        let anchor = pos2(r.right() + 8.0, r.bottom() + 8.0);
        let frame = place(anchor, size, screen_of(screens, anchor));
        let builder = ViewportBuilder::default()
            .with_title("Tidy · créer une boîte")
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_has_shadow(true)
            .with_taskbar(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_active(false)
            .with_position(frame.min)
            .with_inner_size(size);
        let mut done = false;
        ctx.show_viewport_immediate(
            ViewportId::from_hash_of("tidy-draw-offer"),
            builder,
            |ui, _| {
                let rect = ui.max_rect();
                let resp = ui.interact(rect, ui.scope_id().with("créer"), Sense::click());
                let fill = if resp.hovered() {
                    theme::ACCENT_2
                } else {
                    theme::ACCENT_2.gamma_multiply(0.92)
                };
                let painter = ui.painter();
                painter.rect_filled(rect, CornerRadius::same(theme::CARD_RADIUS), fill);
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    format!("{}  {}", icons::DRAW_BOX, t!("box.create_here")),
                    theme::strong(theme::SIZE_M),
                    theme::ON_ACCENT_2,
                );
                if resp.clicked() {
                    out.push(Action::NewBox {
                        near: None,
                        rect: Some(r),
                    });
                    done = true;
                }
                if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                    done = true;
                }
            },
        );
        if done {
            self.draw = None;
        }
    }
}

/// Fond d'une fenêtre flottante.
fn card(painter: &egui::Painter, rect: Rect) {
    let radius = CornerRadius::same(12);
    painter.rect_filled(rect, radius, theme::SURFACE);
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );
}

/// Confirmation : `Some(true)` confirmé (bouton ou Entrée), `Some(false)` annulé.
fn confirm_ui(
    ui: &mut egui::Ui,
    title: &str,
    body: &str,
    confirm: &str,
    danger: bool,
) -> Option<bool> {
    let mut answer = None;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.set_width(CONFIRM_WIDTH - 2.0 * menu::PADDING - 16.0);
            ui.add(
                egui::Label::new(egui::RichText::new(title).font(theme::strong(theme::SIZE_L)))
                    .wrap(),
            );
            if !body.is_empty() {
                ui.add(egui::Label::new(egui::RichText::new(body).color(theme::TEXT_DIM)).wrap());
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let clicked = if danger {
                    widgets::danger(ui, confirm).clicked()
                } else {
                    widgets::primary(ui, confirm).clicked()
                };
                if clicked {
                    answer = Some(true);
                }
                if ui.button(t!("common.cancel")).clicked() {
                    answer = Some(false);
                }
            });
        });
    });
    if answer.is_none() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
        answer = Some(true);
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popups_stay_on_screen() {
        let screen = geometry::Rect::new(0.0, 25.0, 1000.0, 775.0);
        let size = vec2(250.0, 300.0);
        // Au milieu : tel quel.
        assert_eq!(
            place(pos2(100.0, 100.0), size, screen).min,
            pos2(100.0, 100.0)
        );
        // En bas à droite : retourné vers le haut et la gauche.
        let r = place(pos2(900.0, 700.0), size, screen);
        assert_eq!(r.min, pos2(650.0, 400.0));
        // Plus grand que l'écran : collé en haut.
        let r = place(pos2(10.0, 30.0), vec2(250.0, 900.0), screen);
        assert_eq!(r.min.y, 25.0);
    }
}
