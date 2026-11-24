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

/// Couleurs proposées pour les boîtes (fond, titre) : neutres de la DA, puis teintes
/// douces. Une pastille « autre » ouvre le sélecteur complet.
pub const SWATCHES: [Color32; 12] = [
    theme::SURFACE,
    theme::SURFACE_HI,
    Color32::from_rgb(0x3a, 0x3a, 0x42),
    theme::TEXT,
    theme::ACCENT,
    theme::ACCENT_2,
    Color32::from_rgb(0x38, 0xbd, 0xf8),
    Color32::from_rgb(0x22, 0xc5, 0x5e),
    Color32::from_rgb(0xfa, 0xcc, 0x15),
    theme::WARNING,
    Color32::from_rgb(0xf4, 0x3f, 0x5e),
    Color32::from_rgb(0xec, 0x48, 0x99),
];

/// Rangée de pastilles [`SWATCHES`] et sélecteur libre. Garde l'opacité de `color`.
/// Renvoie `true` si la couleur a changé.
pub fn swatches(ui: &mut Ui, color: &mut Color32) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        let alpha = color.a();
        let opaque = |c: Color32| Color32::from_rgb(c.r(), c.g(), c.b());
        for swatch_color in SWATCHES {
            let selected = opaque(*color) == swatch_color;
            let response = swatch(ui, swatch_color, selected);
            if !selected {
                // Les teintes sombres restent visibles sur une carte sombre.
                ui.painter().rect_stroke(
                    response.rect,
                    6,
                    Stroke::new(1.0, theme::BORDER),
                    StrokeKind::Inside,
                );
            }
            if response.clicked() && !selected {
                *color = Color32::from_rgba_unmultiplied(
                    swatch_color.r(),
                    swatch_color.g(),
                    swatch_color.b(),
                    alpha,
                );
                changed = true;
            }
        }
        let mut rgb = [color.r(), color.g(), color.b()];
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            *color = Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], alpha);
            changed = true;
        }
    });
    changed
}

/// Interrupteur (lime quand il est activé). Renvoie une réponse `changed()` au clic.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let size = egui::vec2(38.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), *on, ""));
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(response.id, *on);
        let enabled = ui.is_enabled();
        let track = if *on {
            theme::ACCENT_2
        } else if response.hovered() {
            Color32::from_rgb(0x30, 0x30, 0x38)
        } else {
            theme::SURFACE_HI
        };
        let track = if enabled {
            track
        } else {
            track.gamma_multiply(0.4)
        };
        let painter = ui.painter();
        painter.rect(
            rect,
            CornerRadius::same(255),
            track,
            Stroke::new(1.0, if *on { track } else { theme::BORDER }),
            StrokeKind::Inside,
        );
        let radius = rect.height() / 2.0 - 3.0;
        let x = egui::lerp(
            (rect.left() + radius + 3.0)..=(rect.right() - radius - 3.0),
            t,
        );
        let knob = if *on {
            theme::ON_ACCENT_2
        } else {
            theme::TEXT_DIM
        };
        painter.circle_filled(egui::pos2(x, rect.center().y), radius, knob);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Ligne de réglage : icône, titre et aide à gauche (l'aide passe à la ligne), contrôle
/// à droite.
pub fn setting_row<R>(
    ui: &mut Ui,
    icon: &str,
    title: &str,
    hint: Option<&str>,
    control: impl FnOnce(&mut Ui) -> R,
) -> R {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let out = control(ui);
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if !icon.is_empty() {
                    ui.label(
                        RichText::new(icon)
                            .size(theme::SIZE_L)
                            .color(theme::TEXT_DIM),
                    );
                    ui.add_space(2.0);
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.add(Label::new(RichText::new(title).size(theme::SIZE_M)).wrap());
                    if let Some(hint) = hint {
                        ui.add(Label::new(dim(hint)).wrap());
                    }
                });
            });
            out
        })
        .inner
    })
    .inner
}

/// Fine ligne de séparation entre les lignes d'une carte.
pub fn row_separator(ui: &mut Ui) {
    ui.add_space(4.0);
    let rect = ui.available_rect_before_wrap();
    let y = rect.top();
    ui.painter().hline(
        rect.left()..=rect.right(),
        y,
        Stroke::new(1.0, theme::BORDER),
    );
    ui.add_space(5.0);
}

/// Pastille de texte avec une croix (extension, étiquette). Renvoie `true` si on a
/// cliqué la croix. D'un seul tenant : passe à la ligne entière dans un
/// `horizontal_wrapped`.
pub fn chip(ui: &mut Ui, text: &str) -> bool {
    let font = egui::FontId::proportional(theme::SIZE_S);
    let label = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), theme::TEXT);
    let cross = ui
        .painter()
        .layout_no_wrap(crate::icons::CLOSE.to_owned(), font, theme::TEXT_DIM);
    let (pad_x, pad_y, gap) = (10.0, 4.0, 6.0);
    let size = egui::vec2(
        pad_x + label.size().x + gap + cross.size().x + pad_x * 0.7,
        label.size().y.max(cross.size().y) + 2.0 * pad_y,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let close_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - cross.size().x - pad_x, rect.top()),
        rect.max,
    );
    let close = ui
        .interact(close_rect, response.id.with("retirer"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect(
            rect,
            CornerRadius::same(255),
            theme::SURFACE_HI,
            Stroke::new(1.0, theme::BORDER),
            StrokeKind::Inside,
        );
        let y = rect.center().y;
        painter.galley(
            egui::pos2(rect.left() + pad_x, y - label.size().y / 2.0),
            label,
            theme::TEXT,
        );
        let color = if close.hovered() {
            theme::DANGER
        } else {
            theme::TEXT_DIM
        };
        painter.galley(
            egui::pos2(
                rect.right() - pad_x * 0.7 - cross.size().x,
                y - cross.size().y / 2.0,
            ),
            cross,
            color,
        );
    }
    close.clicked()
}

/// Bouton discret fait d'une icône seule, avec une info-bulle.
pub fn icon_button(ui: &mut Ui, icon: &str, tooltip: &str) -> Response {
    let button = Button::new(RichText::new(icon).size(theme::SIZE_L)).frame(false);
    ui.add(button)
        .on_hover_text(tooltip)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Points d'avancement d'un assistant : `count` étapes, `current` en lime (allongé).
pub fn step_dots(ui: &mut Ui, count: usize, current: usize) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for i in 0..count {
            let width = if i == current { 22.0 } else { 8.0 };
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 8.0), Sense::hover());
            let color = if i == current {
                theme::ACCENT_2
            } else if i < current {
                theme::ACCENT
            } else {
                theme::BORDER
            };
            ui.painter()
                .rect_filled(rect, CornerRadius::same(255), color);
        }
    });
}

/// Grande carte à choisir (assistant) : icône, titre et texte. Bordure lime si elle
/// est choisie. `width` : largeur de la carte, marges comprises.
pub fn choice_card(
    ui: &mut Ui,
    selected: bool,
    width: f32,
    icon: &str,
    title: &str,
    body: &str,
) -> Response {
    let stroke = if selected {
        Stroke::new(2.0, theme::ACCENT_2)
    } else {
        Stroke::new(1.0, theme::BORDER)
    };
    let inner = egui::Frame::new()
        .fill(if selected {
            theme::SURFACE_HI
        } else {
            theme::SURFACE
        })
        .stroke(stroke)
        .corner_radius(CornerRadius::same(theme::CARD_RADIUS))
        .inner_margin(18)
        .show(ui, |ui| {
            ui.set_width(width - 36.0 - 2.0 * stroke.width);
            ui.set_min_height(150.0);
            ui.vertical(|ui| {
                let color = if selected {
                    theme::ACCENT_2
                } else {
                    theme::ACCENT
                };
                ui.label(RichText::new(icon).size(28.0).color(color));
                ui.add_space(6.0);
                ui.label(RichText::new(title).bold().size(theme::SIZE_L));
                ui.add_space(2.0);
                ui.add(Label::new(RichText::new(body).color(theme::TEXT_DIM)).wrap());
            });
        });
    ui.interact(
        inner.response.rect,
        Id::unique(("carte-choix", title)),
        Sense::click(),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Bandeau d'information (fond violet discret), avec icône.
pub fn notice(ui: &mut Ui, color: Color32, icon: &str, text: &str) -> Response {
    let inner = ui.available_width() - 2.0 * (12.0 + 1.0);
    egui::Frame::new()
        .fill(color.gamma_multiply(0.15))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.6)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(inner.max(0.0));
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).color(color));
                ui.add(Label::new(text).wrap());
            });
        })
        .response
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
