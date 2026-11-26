//! Dessin et interactions d'une fenêtre de boîte : barre de titre (menu ☰, nom,
//! enroulement), onglets, fil d'Ariane d'un portail, contenu, bords.
//!
//! La fenêtre a la taille de la boîte : la carte occupe toute la fenêtre. Le système
//! déplace et redimensionne la fenêtre lui-même (`StartDrag`, `BeginResize`), ce qui
//! reste fluide même quand l'interface ne se redessine pas. Une boîte gelée
//! (`DeskBox::frozen`) ne demande ni l'un ni l'autre.

use std::time::{Duration, Instant};

use egui::viewport::ResizeDirection;
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Key, Modifiers, Rect, Sense, Stroke,
    StrokeKind, TextEdit, Ui, ViewportCommand, pos2, vec2,
};
use tidy_core::model::{Appearance, DeskBox, Rgba, TitleVisibility};
use tidy_ui::{icons, t, theme, widgets};

use super::action::Action;
use super::items::{self, GridGeom, ItemDir, ItemsState, ItemsView, Section};
use super::search::{self, SearchEdit};
use super::textures::IconCache;

/// Hauteur de la barre de titre.
pub const TITLE_HEIGHT: f32 = 32.0;
/// Hauteur de la barre d'onglets et du fil d'Ariane.
const BAR_HEIGHT: f32 = 28.0;
/// Largeur de la zone de redimensionnement le long des bords.
const EDGE: f32 = 6.0;
/// Taille minimale d'une boîte déroulée.
pub const MIN_SIZE: [f32; 2] = [160.0, 90.0];
/// Durée d'un message dans une boîte.
const NOTICE: Duration = Duration::from_secs(5);
/// Fond « verre dépoli » actif : opacité du fond multipliée par ce facteur, pour
/// laisser voir le flou du système.
const GLASS_ALPHA: f32 = 0.45;

/// Couleur d'apparence, opacité multipliée par `alpha`.
pub fn color(c: Rgba, alpha: f32) -> Color32 {
    let a = (f32::from(c.3) * alpha.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgba_unmultiplied(c.0, c.1, c.2, a)
}

/// Édition d'un nom (boîte ou onglet) dans la barre.
#[derive(Debug, Clone, PartialEq)]
pub struct NameEdit {
    pub text: String,
    pub focus: bool,
}

/// État d'interface d'une boîte.
#[derive(Debug, Default)]
pub struct BoxUi {
    pub items: ItemsState,
    pub renaming_title: Option<NameEdit>,
    pub renaming_tab: Option<(usize, NameEdit)>,
    notice: Option<(String, Instant)>,
    /// Champ de recherche ouvert dans la barre de titre.
    pub search: Option<SearchEdit>,
}

impl BoxUi {
    /// Message affiché quelques secondes en bas de la boîte.
    pub fn notice(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), Instant::now()));
    }

    /// Un nom est en cours d'édition, ou une recherche ouverte (la boîte ne doit pas
    /// s'enrouler).
    pub fn editing(&self) -> bool {
        self.renaming_title.is_some()
            || self.renaming_tab.is_some()
            || self.items.renaming.is_some()
            || self.search.is_some()
    }

    /// Texte cherché (replié), si la recherche est ouverte et non vide.
    pub fn search_query(&self) -> Option<String> {
        self.search.as_ref().and_then(SearchEdit::query)
    }
}

/// Fil d'Ariane d'un portail.
pub struct PortalView {
    pub crumbs: Vec<String>,
    pub error: bool,
}

/// Ce qu'il faut pour dessiner une boîte.
pub struct BoxView<'a> {
    pub b: &'a DeskBox,
    pub look: &'a Appearance,
    pub sections: &'a [Section],
    /// Dossier des éléments ; `None` : bureau introuvable.
    pub dir: Option<&'a ItemDir>,
    pub portal: Option<PortalView>,
    /// Affichée enroulée (choix de l'utilisateur ou enroulement automatique).
    pub rolled: bool,
    /// Cible d'un glisser interne en cours.
    pub drop_target: bool,
    pub single_click: bool,
    /// Fond « verre dépoli » du système actif derrière la fenêtre.
    pub glass: bool,
}

/// Ce que la boîte rapporte en plus des actions.
#[derive(Debug, Default)]
pub struct BoxOutput {
    /// La souris est sur la fenêtre.
    pub hovered: bool,
    /// Un clic a eu lieu dans la fenêtre.
    pub pressed: bool,
    /// Un bouton a été relâché dans la fenêtre.
    pub released: bool,
    /// Un élément est glissé depuis cette boîte.
    pub dragging: bool,
    /// Maj enfoncée (pour ignorer l'aimantation).
    pub shift: bool,
    /// Grille libre affichée (repère de la fenêtre).
    pub grid: Option<GridGeom>,
}

/// Bord ou coin sous `p`, s'il y en a un.
fn edge_at(rect: Rect, p: egui::Pos2) -> Option<ResizeDirection> {
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

/// Hauteur réservée au titre : toujours, au survol (place gardée), jamais (sauf
/// enroulée, sinon il ne resterait rien).
pub fn title_height(look: &Appearance, rolled: bool) -> f32 {
    match look.title_visibility {
        TitleVisibility::Never if !rolled => 0.0,
        _ => TITLE_HEIGHT,
    }
}

pub fn draw(
    ui: &mut Ui,
    v: &BoxView,
    st: &mut BoxUi,
    icons: &mut IconCache,
    out: &mut Vec<Action>,
) -> BoxOutput {
    let ctx = ui.ctx().clone();
    let b = v.b;
    let id = b.id;
    let look = v.look;
    let rect = ui.max_rect();
    let mut result = BoxOutput {
        hovered: ctx.input(|i| i.pointer.hover_pos().is_some()),
        pressed: ctx.input(|i| i.pointer.any_pressed()),
        released: ctx.input(|i| i.pointer.any_released()),
        shift: ctx.input(|i| i.modifiers.shift),
        ..Default::default()
    };
    let painter = ui.painter().clone();

    // Fond, avec l'opacité « souris loin » quand la souris est ailleurs.
    let opacity = ctx.animate_value_with_time(
        ui.scope_id().with("opacité"),
        if result.hovered || v.drop_target {
            look.opacity
        } else {
            look.opacity_away
        },
        0.18,
    );
    // Verre : le fond est plus transparent pour laisser voir le flou.
    let opacity = if v.glass {
        opacity * GLASS_ALPHA
    } else {
        opacity
    };
    let r = look.corner_radius.clamp(0.0, 40.0) as u8;
    let radius = CornerRadius::same(r);
    painter.rect_filled(rect, radius, color(look.background, opacity));
    let hovering_files = ctx.input(|i| !i.raw.hovered_files.is_empty());
    if v.drop_target || hovering_files {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(2.0, theme::ACCENT_2),
            StrokeKind::Inside,
        );
    } else if look.border {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, theme::BORDER),
            StrokeKind::Inside,
        );
    }

    // Recherche : fermée si l'option a été retirée, si la boîte est enroulée ou s'il
    // n'y a pas de barre de titre pour l'afficher ; Ctrl+F (⌘F) l'ouvre.
    let title_h = title_height(look, v.rolled);
    if !b.search_in_title || title_h == 0.0 || v.rolled {
        st.search = None;
    } else if st.renaming_title.is_none()
        && ctx.input(|i| i.viewport().focused.unwrap_or(false))
        && ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F))
    {
        st.search.get_or_insert_with(SearchEdit::new).focus = true;
    }

    // Barre de titre.
    let title_rect = Rect::from_min_size(rect.min, vec2(rect.width(), title_h));
    let show_title = match look.title_visibility {
        TitleVisibility::Always => true,
        TitleVisibility::OnHover => {
            result.hovered || v.rolled || st.renaming_title.is_some() || st.search.is_some()
        }
        TitleVisibility::Never => v.rolled,
    };
    if title_h > 0.0 {
        title_bar(ui, v, st, title_rect, show_title, opacity, &mut result, out);
    }
    if v.rolled {
        return result;
    }

    // Onglets, fil d'Ariane.
    let mut top = title_rect.bottom();
    if b.tabs.len() > 1 {
        let bar = Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), BAR_HEIGHT));
        tab_bar(ui, v, st, bar, out);
        top = bar.bottom();
    }
    if let Some(portal) = &v.portal {
        let bar = Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), BAR_HEIGHT));
        crumbs(ui, id, portal, bar, out);
        top = bar.bottom();
    }

    // Contenu. Le fond reçoit les clics qui ne tombent sur aucun élément.
    let body = Rect::from_min_max(pos2(rect.left(), top), rect.max);
    let background = ui.interact(
        body.shrink(EDGE),
        ui.scope_id().with("fond"),
        if title_h == 0.0 {
            Sense::click_and_drag()
        } else {
            Sense::click()
        },
    );
    if background.clicked() {
        st.items.selected.clear();
    }
    if background.secondary_clicked() {
        out.push(Action::BoxMenu { b: id });
    }
    // Sans barre de titre : on déplace la boîte par son fond (pas si elle est gelée).
    if background.drag_started() && !b.frozen {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    let query = st.search_query();
    let empty = v.sections.iter().all(|s| s.items.is_empty());
    let no_match = !empty
        && query.as_deref().is_some_and(|q| {
            !v.sections
                .iter()
                .flat_map(|s| s.items.iter())
                .any(|i| search::matches(&i.name, q))
        });
    let mut hovered_item = None;
    if let Some(dir) = v.dir {
        let content = body.shrink2(vec2(6.0, 4.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content));
        child.set_clip_rect(content.intersect(ui.clip_rect()));
        let view = ItemsView {
            b: id,
            sections: v.sections,
            view: b.view,
            look,
            dir,
            single_click: v.single_click,
            portal: v.portal.is_some(),
            free: items::free_grid(b),
            filter: query.as_deref(),
        };
        let r = items::show(&mut child, &view, &mut st.items, icons, out);
        result.dragging = r.dragging;
        result.grid = r.grid;
        hovered_item = r.hovered;
    }
    // Fin d'un glisser interne : l'élément visé sert de point d'insertion.
    for action in out.iter_mut() {
        if let Action::EndDrag { b: from, before } = action
            && *from == id
        {
            before.clone_from(&hovered_item);
        }
    }
    if empty || no_match {
        let hint = match &v.portal {
            _ if no_match => t!("box_extra.search_none"),
            Some(p) if p.error => t!("box.portal_unreadable"),
            Some(_) => t!("box.portal_empty"),
            None => t!("box.empty"),
        };
        let galley = painter.layout(
            hint.to_owned(),
            FontId::proportional(theme::SIZE_S),
            theme::TEXT_DIM,
            body.width() - 24.0,
        );
        let at = body.center() - galley.size() / 2.0;
        painter
            .with_clip_rect(body)
            .galley(at, galley, theme::TEXT_DIM);
    }

    // Fichiers déposés.
    let dropped: Vec<_> = ctx.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .map(|f| f.path().to_path_buf())
            .collect()
    });
    if !dropped.is_empty() {
        out.push(Action::DropFiles {
            b: id,
            paths: dropped,
            before: hovered_item.clone(),
        });
    }

    // Message.
    if let Some((text, since)) = &st.notice {
        let left = NOTICE.saturating_sub(since.elapsed());
        if left.is_zero() {
            st.notice = None;
        } else {
            ctx.request_repaint_after(left);
            notice(&painter, body, text);
        }
    }

    resize_edges(&ctx, rect, &result, v.rolled || b.frozen);
    result
}

#[allow(clippy::too_many_arguments)]
fn title_bar(
    ui: &mut Ui,
    v: &BoxView,
    st: &mut BoxUi,
    bar: Rect,
    show: bool,
    opacity: f32,
    result: &mut BoxOutput,
    out: &mut Vec<Action>,
) {
    let ctx = ui.ctx().clone();
    let b = v.b;
    let id = b.id;
    let look = v.look;
    let painter = ui.painter().clone();
    let r = look.corner_radius.clamp(0.0, 40.0) as u8;
    let inner = r.saturating_sub(1);
    let bottom = if v.rolled { inner } else { 0 };
    let shape = CornerRadius {
        nw: inner,
        ne: inner,
        sw: bottom,
        se: bottom,
    };

    // Zone de déplacement (tout le titre, sous les boutons).
    let drag = ui.interact(
        bar.shrink(EDGE / 2.0),
        ui.scope_id().with("titre"),
        Sense::click_and_drag(),
    );
    if show {
        painter.rect_filled(
            bar.shrink(1.0),
            shape,
            color(look.title_background, opacity),
        );
        if drag.hovered() || drag.dragged() {
            painter.rect_filled(bar.shrink(1.0), shape, theme::TEXT.gamma_multiply(0.04));
        }
        if !v.rolled {
            painter.line_segment(
                [bar.left_bottom(), bar.right_bottom()],
                Stroke::new(1.0, theme::BORDER.gamma_multiply(opacity.max(0.3))),
            );
        }
    }
    // Gelée : la barre ne déplace plus la boîte.
    if drag.drag_started() && !b.frozen {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if drag.dragged() && !b.frozen {
        ctx.set_cursor_icon(CursorIcon::Grabbing);
    }
    if drag.double_clicked() {
        out.push(Action::ToggleRoll { b: id });
    }
    if drag.secondary_clicked() {
        out.push(Action::BoxMenu { b: id });
    }
    if !show {
        return;
    }

    let title_color = color(look.title_color, 1.0);
    // Menu ☰.
    let menu = Rect::from_center_size(bar.left_center() + vec2(18.0, 0.0), vec2(26.0, 24.0));
    let m = ui.interact(menu, ui.scope_id().with("menu"), Sense::click());
    if m.hovered() {
        painter.rect_filled(menu, CornerRadius::same(8), theme::TEXT.gamma_multiply(0.1));
    }
    painter.text(
        menu.center(),
        Align2::CENTER_CENTER,
        if matches!(b.kind, tidy_core::model::BoxKind::Portal { .. }) {
            icons::PORTAL
        } else {
            icons::MENU
        },
        FontId::proportional(theme::SIZE_M),
        if m.hovered() {
            theme::ACCENT_2
        } else {
            title_color
        },
    );
    if m.clicked() || m.secondary_clicked() {
        out.push(Action::BoxMenu { b: id });
    }

    // À droite : enrouler / dérouler (au survol, ou si enroulée), recherche, gel.
    // L'emplacement du bouton d'enroulement est gardé dès qu'il y a autre chose à
    // droite, pour que la loupe ne bouge pas au survol.
    let slot =
        |right: f32| Rect::from_center_size(pos2(right - 13.0, bar.center().y), vec2(26.0, 24.0));
    let roll = slot(bar.right() - 5.0);
    let searchable = b.search_in_title && !v.rolled;
    let mut right = bar.right() - 5.0;
    if result.hovered || b.rolled_up {
        let rr = ui.interact(roll, ui.scope_id().with("enrouler"), Sense::click());
        if rr.hovered() {
            painter.rect_filled(roll, CornerRadius::same(8), theme::TEXT.gamma_multiply(0.1));
        }
        painter.text(
            roll.center(),
            Align2::CENTER_CENTER,
            if b.rolled_up {
                icons::ROLL_DOWN
            } else {
                icons::ROLL_UP
            },
            FontId::proportional(theme::SIZE_M),
            if rr.hovered() {
                theme::ACCENT_2
            } else {
                theme::TEXT_DIM
            },
        );
        if rr.clicked() {
            out.push(Action::ToggleRoll { b: id });
        }
        right = roll.left() - 2.0;
    } else if searchable || b.frozen {
        right = roll.left() - 2.0;
    }
    if searchable {
        let loupe = slot(right);
        let lr = ui.interact(loupe, ui.scope_id().with("loupe"), Sense::click());
        let active = st.search.is_some();
        if lr.hovered() || active {
            painter.rect_filled(
                loupe,
                CornerRadius::same(8),
                theme::TEXT.gamma_multiply(if active { 0.14 } else { 0.1 }),
            );
        }
        painter.text(
            loupe.center(),
            Align2::CENTER_CENTER,
            icons::SEARCH,
            FontId::proportional(theme::SIZE_M),
            if lr.hovered() || active {
                theme::ACCENT_2
            } else {
                theme::TEXT_DIM
            },
        );
        if lr.clicked() {
            if active {
                st.search = None;
            } else {
                st.search = Some(SearchEdit::new());
                ctx.send_viewport_cmd(ViewportCommand::Focus);
            }
        }
        lr.on_hover_text(t!("box_extra.search_tooltip"));
        right = loupe.left() - 2.0;
    }
    if b.frozen {
        let lock = Rect::from_center_size(pos2(right - 9.0, bar.center().y), vec2(18.0, 24.0));
        painter.text(
            lock.center(),
            Align2::CENTER_CENTER,
            icons::LOCK,
            FontId::proportional(theme::SIZE_S),
            theme::TEXT_DIM,
        );
        ui.interact(lock, ui.scope_id().with("gel"), Sense::hover())
            .on_hover_text(t!("box_extra.frozen_tooltip"));
        right = lock.left() - 2.0;
    }
    let name_right = (right - 4.0).max(menu.right() + 40.0);

    // Nom, ou son édition.
    let name_rect = Rect::from_min_max(
        pos2(menu.right() + 6.0, bar.top() + 4.0),
        pos2(name_right, bar.bottom() - 4.0),
    );
    if st.renaming_title.is_none()
        && searchable
        && let Some(edit) = st.search.as_mut()
    {
        let resp = ui.put(
            name_rect,
            TextEdit::singleline(&mut edit.text)
                .font(FontId::proportional(theme::SIZE_S))
                .hint_text(t!("box_extra.search_hint"))
                .margin(vec2(6.0, 2.0)),
        );
        if edit.focus {
            edit.focus = false;
            resp.request_focus();
        }
        // Échap vide et ferme ; un champ vide qui perd le focus se ferme aussi.
        if resp.lost_focus() {
            let cancel = ui.input(|i| i.key_pressed(Key::Escape));
            if cancel || edit.text.trim().is_empty() {
                st.search = None;
            }
        }
    } else if let Some(edit) = st.renaming_title.as_mut() {
        let resp = ui.put(
            name_rect,
            TextEdit::singleline(&mut edit.text)
                .font(theme::strong(look.title_size))
                .margin(vec2(4.0, 1.0)),
        );
        if edit.focus {
            edit.focus = false;
            resp.request_focus();
        }
        if resp.lost_focus() {
            let cancel = ui.input(|i| i.key_pressed(Key::Escape));
            if let Some(edit) = st.renaming_title.take()
                && !cancel
            {
                out.push(Action::RenameBox {
                    b: id,
                    name: edit.text.trim().to_owned(),
                });
            }
        }
    } else {
        let count: usize = v.sections.iter().map(|s| s.items.len()).sum();
        let mut name = b.name.clone();
        if name.is_empty() {
            name = t!("box.untitled").to_owned();
        }
        let font = theme::strong(look.title_size.clamp(9.0, 24.0));
        let max_chars = ((name_rect.width() / (font.size * 0.55)).floor() as usize).max(3);
        let text = widgets::truncate(&name, max_chars);
        let painted = painter.with_clip_rect(name_rect).text(
            name_rect.left_center(),
            Align2::LEFT_CENTER,
            text,
            font,
            title_color,
        );
        // Enroulée : le nombre d'éléments, pour savoir ce qu'elle cache.
        if v.rolled && count > 0 && painted.right() + 30.0 < name_rect.right() {
            painter.text(
                pos2(painted.right() + 8.0, name_rect.center().y),
                Align2::LEFT_CENTER,
                count.to_string(),
                FontId::proportional(theme::SIZE_XS),
                theme::TEXT_DIM,
            );
        }
    }
}

fn tab_bar(ui: &mut Ui, v: &BoxView, st: &mut BoxUi, bar: Rect, out: &mut Vec<Action>) {
    let b = v.b;
    let painter = ui.painter().clone();
    let font = FontId::proportional(theme::SIZE_S);
    let mut x = bar.left() + 8.0;
    for (i, tab) in b.tabs.iter().enumerate() {
        let label = if tab.name.is_empty() {
            t!("box.tab_untitled", n = i + 1)
        } else {
            widgets::truncate(&tab.name, 18)
        };
        let w = painter
            .layout_no_wrap(label.clone(), font.clone(), theme::TEXT)
            .size()
            .x
            + 20.0;
        let chip = Rect::from_min_size(pos2(x, bar.top() + 3.0), vec2(w, BAR_HEIGHT - 6.0));
        x = chip.right() + 4.0;
        if chip.left() > bar.right() - 8.0 {
            break;
        }
        let active = i == b.active_tab;
        if let Some((index, edit)) = st.renaming_tab.as_mut()
            && *index == i
        {
            let field = chip.with_max_x(chip.right().max(chip.left() + 110.0));
            let resp = ui.put(
                field,
                TextEdit::singleline(&mut edit.text)
                    .font(font.clone())
                    .margin(vec2(6.0, 1.0)),
            );
            if edit.focus {
                edit.focus = false;
                resp.request_focus();
            }
            if resp.lost_focus() {
                let cancel = ui.input(|inp| inp.key_pressed(Key::Escape));
                if let Some((index, edit)) = st.renaming_tab.take()
                    && !cancel
                {
                    out.push(Action::RenameTab {
                        b: b.id,
                        index,
                        name: edit.text.trim().to_owned(),
                    });
                }
            }
            x = field.right() + 4.0;
            continue;
        }
        let resp = ui.interact(chip, ui.scope_id().with(("onglet", i)), Sense::click());
        let (fill, text) = if active {
            (theme::ACCENT_2, theme::ON_ACCENT_2)
        } else if resp.hovered() {
            (theme::SURFACE_HI, theme::TEXT)
        } else {
            (Color32::TRANSPARENT, theme::TEXT_DIM)
        };
        painter.rect_filled(chip, CornerRadius::same(10), fill);
        painter.with_clip_rect(bar).text(
            chip.center(),
            Align2::CENTER_CENTER,
            label,
            font.clone(),
            text,
        );
        if resp.clicked() && !active {
            out.push(Action::SelectTab { b: b.id, index: i });
        }
        if resp.double_clicked() {
            st.renaming_tab = Some((
                i,
                NameEdit {
                    text: tab.name.clone(),
                    focus: true,
                },
            ));
        }
        if resp.secondary_clicked() {
            out.push(Action::BoxMenu { b: b.id });
        }
    }
}

fn crumbs(
    ui: &mut Ui,
    b: tidy_core::model::BoxId,
    portal: &PortalView,
    bar: Rect,
    out: &mut Vec<Action>,
) {
    let painter = ui.painter().clone();
    let font = FontId::proportional(theme::SIZE_S);
    let depth = portal.crumbs.len().saturating_sub(1);
    let mut x = bar.left() + 8.0;
    // Retour.
    let back = Rect::from_min_size(pos2(x, bar.top() + 3.0), vec2(24.0, BAR_HEIGHT - 6.0));
    let enabled = depth > 0;
    let r = ui.interact(
        back,
        ui.scope_id().with("retour"),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if enabled && r.hovered() {
        painter.rect_filled(back, CornerRadius::same(8), theme::SURFACE_HI);
    }
    painter.text(
        back.center(),
        Align2::CENTER_CENTER,
        icons::BACK,
        font.clone(),
        if enabled { theme::TEXT } else { theme::BORDER },
    );
    if enabled && r.clicked() {
        out.push(Action::PortalUp {
            b,
            depth: depth - 1,
        });
    }
    x = back.right() + 6.0;
    // Fil d'Ariane : les derniers niveaux qui tiennent, « … » devant sinon.
    let sep = format!(" {} ", icons::CHEVRON_RIGHT);
    let widths: Vec<f32> = portal
        .crumbs
        .iter()
        .map(|c| {
            painter
                .layout_no_wrap(widgets::truncate(c, 22), font.clone(), theme::TEXT)
                .size()
                .x
        })
        .collect();
    let sep_w = painter
        .layout_no_wrap(sep.clone(), font.clone(), theme::TEXT)
        .size()
        .x;
    let available = bar.right() - 8.0 - x;
    let mut first = portal.crumbs.len().saturating_sub(1);
    let mut used = widths.last().copied().unwrap_or(0.0);
    while first > 0 && used + sep_w + widths[first - 1] + 20.0 <= available {
        first -= 1;
        used += sep_w + widths[first];
    }
    let clip = painter.with_clip_rect(bar);
    if first > 0 {
        let r = clip.text(
            pos2(x, bar.center().y),
            Align2::LEFT_CENTER,
            "…",
            font.clone(),
            theme::TEXT_DIM,
        );
        x = r.right();
        let r = clip.text(
            pos2(x, bar.center().y),
            Align2::LEFT_CENTER,
            &sep,
            font.clone(),
            theme::TEXT_DIM,
        );
        x = r.right();
    }
    for (i, crumb) in portal.crumbs.iter().enumerate().skip(first) {
        let last = i == depth;
        let text = widgets::truncate(crumb, 22);
        let rect = Rect::from_min_size(pos2(x, bar.top() + 3.0), vec2(widths[i], BAR_HEIGHT - 6.0));
        let resp = ui.interact(
            rect,
            ui.scope_id().with(("ariane", i)),
            if last { Sense::hover() } else { Sense::click() },
        );
        let color = if last {
            theme::TEXT
        } else if resp.hovered() {
            theme::ACCENT_2
        } else {
            theme::TEXT_DIM
        };
        clip.text(
            rect.left_center(),
            Align2::LEFT_CENTER,
            text,
            font.clone(),
            color,
        );
        if !last && resp.clicked() {
            out.push(Action::PortalUp { b, depth: i });
        }
        x = rect.right();
        if !last {
            let r = clip.text(
                pos2(x, bar.center().y),
                Align2::LEFT_CENTER,
                &sep,
                font.clone(),
                theme::TEXT_DIM,
            );
            x = r.right();
        }
    }
    painter.line_segment(
        [
            bar.left_bottom() + vec2(8.0, 0.0),
            bar.right_bottom() - vec2(8.0, 0.0),
        ],
        Stroke::new(1.0, theme::BORDER),
    );
}

fn notice(painter: &egui::Painter, body: Rect, text: &str) {
    let width = (body.width() - 20.0).max(60.0);
    let galley = painter.layout(
        text.to_owned(),
        FontId::proportional(theme::SIZE_S),
        theme::TEXT,
        width - 20.0,
    );
    let size = galley.size() + vec2(20.0, 12.0);
    let rect = Rect::from_center_size(
        pos2(body.center().x, body.bottom() - 10.0 - size.y / 2.0),
        size,
    );
    painter.rect_filled(rect, CornerRadius::same(10), theme::SURFACE_HI);
    painter.rect_stroke(
        rect,
        CornerRadius::same(10),
        Stroke::new(1.0, theme::ACCENT),
        StrokeKind::Inside,
    );
    painter.galley(rect.min + vec2(10.0, 6.0), galley, theme::TEXT);
}

/// Bords : redimensionner la fenêtre (pas quand elle est enroulée).
fn resize_edges(ctx: &egui::Context, rect: Rect, result: &BoxOutput, rolled: bool) {
    if rolled || result.dragging {
        return;
    }
    if let Some(pointer) = ctx.pointer_hover_pos()
        && let Some(direction) = edge_at(rect, pointer)
    {
        ctx.set_cursor_icon(cursor_for(direction));
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, Pos2, RawInput};

    /// Commandes de fenêtre demandées par une boîte quand on appuie en `from` puis
    /// qu'on glisse jusqu'à `to`.
    fn commands_for_drag(frozen: bool, from: Pos2, to: Pos2) -> Vec<ViewportCommand> {
        let ctx = egui::Context::default();
        let mut b = DeskBox::new("A");
        b.frozen = frozen;
        let look = Appearance::default();
        let view = BoxView {
            b: &b,
            look: &look,
            sections: &[],
            dir: None,
            portal: None,
            rolled: false,
            drop_target: false,
            single_click: false,
            glass: false,
        };
        let mut st = BoxUi::default();
        let mut icons = IconCache::new(ctx.clone());
        let press = |pressed| Event::PointerButton {
            pos: from,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let frames = [
            vec![Event::PointerMoved(from)],
            vec![press(true)],
            vec![Event::PointerMoved(from.lerp(to, 0.5))],
            vec![Event::PointerMoved(to)],
            vec![Event::PointerMoved(to), press(false)],
        ];
        let mut commands = Vec::new();
        for events in frames {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(300.0, 200.0))),
                events,
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                let mut out = Vec::new();
                draw(ui, &view, &mut st, &mut icons, &mut out);
            });
            for viewport in output.viewport_output.values() {
                commands.extend(viewport.commands.iter().cloned());
            }
            output.drop_without_applying_deltas();
        }
        commands
    }

    /// Éléments sélectionnés par Ctrl+A (⌘A) dans une boîte qui affiche `names`, avec
    /// `search` tapé dans la barre de titre.
    fn select_all_with_search(view_mode: tidy_core::model::ViewMode, search: &str) -> Vec<String> {
        use tidy_core::category::Category;
        use tidy_core::rules::ItemInfo;
        let ctx = egui::Context::default();
        let dir = tempfile::tempdir().unwrap();
        let dir = ItemDir::plain(dir.path().to_path_buf());
        let mut b = DeskBox::new("A");
        b.search_in_title = true;
        b.view = view_mode;
        let names = ["Été 2026.jpg", "notes.txt", "ETE.pdf", "budget.xlsx"];
        let info = |name: &str| ItemInfo {
            name: name.into(),
            is_dir: false,
            category: Category::Other,
            size: 0,
            modified: None,
            uti: None,
        };
        let sections = [Section {
            group: None,
            items: names.iter().map(|n| info(n)).collect(),
        }];
        let look = Appearance::default();
        let view = BoxView {
            b: &b,
            look: &look,
            sections: &sections,
            dir: Some(&dir),
            portal: None,
            rolled: false,
            drop_target: false,
            single_click: false,
            glass: false,
        };
        let mut st = BoxUi {
            search: Some(SearchEdit {
                text: search.into(),
                focus: false,
            }),
            ..Default::default()
        };
        let mut icons = IconCache::new(ctx.clone());
        let mut input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0))),
            events: vec![Event::Key {
                key: Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::COMMAND,
            }],
            ..Default::default()
        };
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .focused = Some(true);
        let output = ctx.run_ui(input, |ui| {
            let mut out = Vec::new();
            draw(ui, &view, &mut st, &mut icons, &mut out);
        });
        output.drop_without_applying_deltas();
        st.items.selected.into_iter().collect()
    }

    #[test]
    fn search_shows_only_matching_items() {
        use tidy_core::model::ViewMode;
        // Grille libre (ordre manuel), liste : seuls les éléments qui correspondent,
        // sans tenir compte de la casse ni des accents, sont affichés et sélectionnés.
        for mode in [ViewMode::Grid, ViewMode::List] {
            assert_eq!(
                select_all_with_search(mode, "ete"),
                ["ETE.pdf", "Été 2026.jpg"],
                "{mode:?}"
            );
            assert_eq!(select_all_with_search(mode, "  ").len(), 4, "{mode:?}");
            assert!(select_all_with_search(mode, "zzz").is_empty(), "{mode:?}");
        }
    }

    #[test]
    fn a_frozen_box_neither_moves_nor_resizes() {
        let title = (pos2(150.0, 16.0), pos2(220.0, 60.0));
        let edge = (pos2(2.0, 100.0), pos2(-40.0, 100.0));
        let moves = |c: &[ViewportCommand]| c.contains(&ViewportCommand::StartDrag);
        let resizes = |c: &[ViewportCommand]| {
            c.iter()
                .any(|c| matches!(c, ViewportCommand::BeginResize(_)))
        };
        // Boîte ordinaire : la barre de titre déplace, le bord redimensionne.
        assert!(moves(&commands_for_drag(false, title.0, title.1)));
        assert!(resizes(&commands_for_drag(false, edge.0, edge.1)));
        // Gelée : ni l'un ni l'autre.
        assert!(!moves(&commands_for_drag(true, title.0, title.1)));
        let at_edge = commands_for_drag(true, edge.0, edge.1);
        assert!(!resizes(&at_edge) && !moves(&at_edge));
    }

    #[test]
    fn edges_and_corners() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 200.0));
        assert_eq!(edge_at(r, pos2(150.0, 100.0)), None);
        assert_eq!(edge_at(r, pos2(2.0, 100.0)), Some(ResizeDirection::West));
        assert_eq!(
            edge_at(r, pos2(298.0, 198.0)),
            Some(ResizeDirection::SouthEast)
        );
        assert_eq!(edge_at(r, pos2(150.0, 1.0)), Some(ResizeDirection::North));
    }

    #[test]
    fn title_space() {
        let mut look = Appearance::default();
        assert_eq!(title_height(&look, false), TITLE_HEIGHT);
        look.title_visibility = TitleVisibility::OnHover;
        assert_eq!(title_height(&look, false), TITLE_HEIGHT);
        look.title_visibility = TitleVisibility::Never;
        assert_eq!(title_height(&look, false), 0.0);
        // Enroulée, le titre reste visible.
        assert_eq!(title_height(&look, true), TITLE_HEIGHT);
    }

    #[test]
    fn colors_take_the_opacity() {
        let c = color(Rgba(10, 20, 30, 200), 0.5);
        assert_eq!(c.a(), 100);
        assert_eq!(color(Rgba(1, 2, 3, 255), 2.0).a(), 255);
    }
}
