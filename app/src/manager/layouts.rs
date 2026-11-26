//! Page « Dispositions » : instantanés (automatiques, manuels, le premier) avec leur
//! miniature, dispositions nommées et leur raccourci, comportement au changement de
//! résolution.
//!
//! La page **lit** l'index (`snapshots/index.json`) et les instantanés sans rien
//! écrire ; toute modification passe par l'agent (module `layouts`), seul écrivain du
//! dossier `snapshots/`. Restaurer passe ensuite par le processus du bureau, seul
//! écrivain de `desktop.json`. Les miniatures sont dessinées à partir des cadres des
//! boîtes sur l'écran principal (pas de capture d'écran).
//!
//! L'état de la page (index lu, miniatures, saisies) vit dans la mémoire d'egui : la
//! structure `Manager` n'a pas à le connaître.

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use egui::{RichText, TextEdit};
use jiff::Timestamp;
use jiff::tz::TimeZone;
use serde_json::{Value, json};
use tidy_core::layouts::{
    self, LayoutIndex, NamedLayout, SnapshotKind, SnapshotMeta, SnapshotReason, ThumbBox, Uuid,
};
use tidy_core::screen::Screen;
use tidy_core::settings::ResolutionPolicy;
use tidy_core::{DesktopCommand, Store};
use tidy_ui::hotkey::{self, FieldChange};
use tidy_ui::{icons, t, theme, widgets};

use super::Manager;
use super::pages::{card_title, page_header};
use super::shortcuts;
use crate::ipc::Request;

/// Relecture de l'index (l'agent prend des instantanés tout seul).
const REFRESH_EVERY: Duration = Duration::from_secs(2);
/// Hauteur des miniatures de l'historique.
const THUMB_HEIGHT: f32 = 64.0;
/// Nombres d'instantanés automatiques proposés.
const KEEP_CHOICES: [usize; 5] = [10, 20, 50, 100, 200];

/// Miniature : proportions de l'écran principal et cadres des boîtes.
#[derive(Debug, Clone)]
struct Thumb {
    ratio: f32,
    boxes: Vec<ThumbBox>,
}

/// Action qui attend une confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Restore(Uuid),
    DeleteSnapshot(Uuid),
    ApplyLayout(Uuid),
    UpdateLayout(Uuid),
    DeleteLayout(Uuid),
    ForgetScreens,
}

/// État de la page, gardé dans la mémoire d'egui d'une image à l'autre.
#[derive(Debug, Clone, Default)]
struct PageState {
    index: LayoutIndex,
    index_modified: Option<SystemTime>,
    read_at: Option<Instant>,
    screens: Vec<Screen>,
    policy: Option<ResolutionPolicy>,
    /// Miniatures par (identifiant, empreinte) ; `None` : fichier illisible.
    thumbs: HashMap<(Uuid, u64), Option<Thumb>>,
    /// Instantané ou disposition en cours de renommage, et le nom saisi.
    renaming: Option<(Uuid, String)>,
    new_snapshot_name: String,
    new_layout_name: String,
    pending: Option<Pending>,
}

impl PageState {
    /// Relit l'index s'il a changé (au plus toutes les [`REFRESH_EVERY`], ou tout de
    /// suite après une demande), et les écrans.
    fn refresh(&mut self, store: &Store, policy: ResolutionPolicy) {
        if self.policy != Some(policy) {
            self.policy = Some(policy);
            self.thumbs.clear();
        }
        if self.read_at.is_some_and(|t| t.elapsed() < REFRESH_EVERY) {
            return;
        }
        self.read_at = Some(Instant::now());
        let modified = std::fs::metadata(store.path_of::<LayoutIndex>())
            .and_then(|m| m.modified())
            .ok();
        let screens = tidy_platform::current().screens();
        if screens != self.screens {
            self.screens = screens;
            self.thumbs.clear();
        }
        if modified == self.index_modified && modified.is_some() {
            return;
        }
        self.index_modified = modified;
        self.index = layouts::peek_index(store);
        let index = &self.index;
        self.thumbs.retain(|(id, hash), _| {
            index.snapshot(*id).is_some_and(|s| s.hash == *hash)
                || index.layout(*id).is_some_and(|l| l.hash == *hash)
        });
    }

    /// Force la relecture à la prochaine image (après une demande à l'agent).
    fn reload(&mut self) {
        self.read_at = None;
        self.index_modified = None;
    }

    fn thumb(&mut self, store: &Store, id: Uuid, hash: u64) -> Option<&Thumb> {
        let (screens, policy) = (&self.screens, self.policy.unwrap_or_default());
        self.thumbs
            .entry((id, hash))
            .or_insert_with(|| {
                let state = layouts::load_state(store, id).ok()?;
                let (ratio, boxes) = layouts::thumbnail(&state, screens, policy);
                Some(Thumb { ratio, boxes })
            })
            .as_ref()
    }
}

/// Demande au module `layouts` de l'agent. Rend `true` si elle a abouti.
fn ask(m: &mut Manager, page: &mut PageState, action: &str, data: Value) -> bool {
    let ok = m.request(Request::Module {
        module: "layouts".into(),
        action: action.into(),
        data,
    });
    page.reload();
    ok
}

pub fn show(m: &mut Manager, ui: &mut egui::Ui) {
    let id = egui::Id::unique("page-dispositions");
    let mut page: PageState = ui.data_mut(|d| d.remove_temp(id)).unwrap_or_default();
    page.refresh(&m.store, m.settings.layout.on_resolution_change);
    ui.ctx().request_repaint_after(REFRESH_EVERY);

    page_header(
        ui,
        icons::SNAPSHOT,
        t!("manager.layouts"),
        t!("layouts.intro"),
    );
    snapshot_settings(m, ui, &mut page);
    ui.add_space(12.0);
    named_layouts(m, ui, &mut page);
    ui.add_space(12.0);
    resolution(m, ui, &mut page);
    ui.add_space(20.0);
    history(m, ui, &mut page);
    confirmation(m, ui.ctx(), &mut page);

    ui.data_mut(|d| d.insert_temp(id, page));
}

/// Réglages des instantanés et « Prendre un instantané ».
fn snapshot_settings(m: &mut Manager, ui: &mut egui::Ui, page: &mut PageState) {
    widgets::card(ui, |ui| {
        card_title(ui, icons::SNAPSHOT, t!("layouts.snapshots_title"));
        let mut auto = m.settings.snapshots.auto;
        if widgets::setting_row(
            ui,
            icons::CLOCK,
            t!("layouts.auto"),
            Some(t!("layouts.auto_hint")),
            |ui| widgets::toggle(ui, &mut auto).changed(),
        ) {
            m.settings.snapshots.auto = auto;
            m.save();
        }
        widgets::row_separator(ui);
        let mut keep = m.settings.snapshots.keep;
        let changed = widgets::setting_row(
            ui,
            icons::LAYOUT_LIST,
            t!("layouts.keep"),
            Some(t!("layouts.keep_hint")),
            |ui| {
                let mut changed = false;
                // De droite à gauche : le plus grand d'abord.
                for value in KEEP_CHOICES.into_iter().rev() {
                    changed |=
                        widgets::choice_value(ui, &mut keep, value, value.to_string()).changed();
                }
                changed
            },
        );
        if changed {
            m.settings.snapshots.keep = keep;
            m.save();
            ask(m, page, "prune", Value::Null);
        }
        widgets::row_separator(ui);
        ui.horizontal_wrapped(|ui| {
            ui.add(
                TextEdit::singleline(&mut page.new_snapshot_name)
                    .hint_text(t!("layouts.snapshot_name_hint"))
                    .desired_width(240.0),
            );
            if widgets::primary(ui, format!("{}  {}", icons::CAMERA, t!("layouts.take")))
                .on_hover_text(t!("layouts.take_hint"))
                .clicked()
            {
                let name = std::mem::take(&mut page.new_snapshot_name);
                if ask(m, page, "take", json!({ "name": name })) {
                    m.show_notice(t!("layouts.taken"));
                }
            }
        });
    });
}

/// Dispositions nommées : enregistrer l'actuelle, appliquer, raccourci, renommer…
fn named_layouts(m: &mut Manager, ui: &mut egui::Ui, page: &mut PageState) {
    widgets::card(ui, |ui| {
        card_title(ui, icons::LAYOUT, t!("layouts.named_title"));
        widgets::hint(ui, t!("layouts.named_hint"));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let response = ui.add(
                TextEdit::singleline(&mut page.new_layout_name)
                    .hint_text(t!("layouts.layout_name_hint"))
                    .desired_width(240.0),
            );
            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let ready = !page.new_layout_name.trim().is_empty();
            let clicked = ui
                .add_enabled_ui(ready, |ui| {
                    widgets::primary(
                        ui,
                        format!("{}  {}", icons::PLUS, t!("layouts.save_layout")),
                    )
                })
                .inner
                .clicked();
            if ready && (clicked || enter) {
                let name = std::mem::take(&mut page.new_layout_name);
                if ask(m, page, "save_layout", json!({ "name": name.trim() })) {
                    m.show_notice(t!("layouts.saved_layout", name = name.trim()));
                }
            }
        });
        let list = page.index.layouts.clone();
        if list.is_empty() {
            ui.add_space(4.0);
            ui.label(widgets::dim(t!("layouts.no_layout")));
        }
        for layout in &list {
            widgets::row_separator(ui);
            layout_row(m, ui, page, layout);
        }
    });
}

fn layout_row(m: &mut Manager, ui: &mut egui::Ui, page: &mut PageState, layout: &NamedLayout) {
    ui.horizontal(|ui| {
        let thumb = page.thumb(&m.store, layout.id, layout.hash).cloned();
        thumbnail(ui, thumb.as_ref(), THUMB_HEIGHT * 0.75);
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon_button(ui, icons::DELETE, t!("layouts.delete_layout")).clicked() {
                page.pending = Some(Pending::DeleteLayout(layout.id));
            }
            if widgets::icon_button(ui, icons::EDIT, t!("common.rename")).clicked() {
                page.renaming = Some((layout.id, layout.name.clone()));
            }
            if widgets::icon_button(ui, icons::REFRESH, t!("layouts.update_hint")).clicked() {
                page.pending = Some(Pending::UpdateLayout(layout.id));
            }
            if widgets::primary(ui, t!("layouts.apply")).clicked() {
                page.pending = Some(Pending::ApplyLayout(layout.id));
            }
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    name_or_editor(m, ui, page, layout.id, &layout.name, "rename_layout");
                    ui.label(widgets::dim(t!(
                        "layouts.layout_meta",
                        count = layout.boxes,
                        date = date(layout.updated)
                    )));
                });
            });
        });
    });
    ui.add_space(4.0);
    hotkey_row(m, ui, page, layout);
}

/// Raccourci d'une disposition, refusé s'il sert déjà ailleurs dans Tidy.
fn hotkey_row(m: &mut Manager, ui: &mut egui::Ui, page: &mut PageState, layout: &NamedLayout) {
    let change = widgets::setting_row(
        ui,
        icons::KEYBOARD,
        t!("layouts.hotkey"),
        Some(t!("layouts.hotkey_hint")),
        |ui| {
            let (settings, desktop, index) = (&m.settings, &m.desktop, &page.index);
            hotkey::field(
                ui,
                ("disposition", layout.id),
                layout.hotkey.as_deref(),
                |text| match hotkey_conflict(settings, desktop, index, layout.id, text) {
                    Some(owner) => Err(t!("hotkey.conflict", owner = owner)),
                    None => Ok(()),
                },
            )
        },
    );
    if let Some(change) = change {
        let value = match change {
            FieldChange::Set(text) => Value::String(text),
            FieldChange::Cleared => Value::Null,
        };
        ask(
            m,
            page,
            "set_layout_hotkey",
            json!({ "id": layout.id, "hotkey": value }),
        );
    }
}

/// Qui utilise déjà `text` : un raccourci des réglages, d'une boîte ou d'une autre
/// disposition.
fn hotkey_conflict(
    settings: &tidy_core::Settings,
    desktop: &tidy_core::DesktopState,
    index: &LayoutIndex,
    me: Uuid,
    text: &str,
) -> Option<String> {
    let others = shortcuts::all(settings, desktop);
    if let Some(slot) = hotkey::conflict(
        text,
        others.iter().map(|(slot, value)| (*slot, value.as_deref())),
    ) {
        return Some(slot.label(desktop));
    }
    hotkey::conflict(
        text,
        index
            .layouts
            .iter()
            .filter(|l| l.id != me)
            .map(|l| (l.name.as_str(), l.hotkey.as_deref())),
    )
    .map(|name| t!("layouts.hotkey_owner", name = name))
}

/// Comportement au changement de résolution ou d'écrans.
fn resolution(m: &mut Manager, ui: &mut egui::Ui, page: &mut PageState) {
    widgets::card(ui, |ui| {
        card_title(ui, icons::MONITOR, t!("layouts.resolution_title"));
        widgets::hint(ui, t!("layouts.resolution_hint"));
        ui.add_space(6.0);
        let mut policy = m.settings.layout.on_resolution_change;
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            for (value, label) in [
                (
                    ResolutionPolicy::RememberPerResolution,
                    t!("layouts.policy_remember"),
                ),
                (ResolutionPolicy::Rescale, t!("layouts.policy_rescale")),
                (ResolutionPolicy::Nothing, t!("layouts.policy_nothing")),
            ] {
                changed |= widgets::choice_value(ui, &mut policy, value, label).changed();
            }
        });
        widgets::hint(
            ui,
            match policy {
                ResolutionPolicy::RememberPerResolution => t!("layouts.policy_remember_hint"),
                ResolutionPolicy::Rescale => t!("layouts.policy_rescale_hint"),
                ResolutionPolicy::Nothing => t!("layouts.policy_nothing_hint"),
            },
        );
        if changed {
            m.settings.layout.on_resolution_change = policy;
            m.save();
        }
        widgets::row_separator(ui);
        let clicked = widgets::setting_row(
            ui,
            icons::RESTORE,
            t!("layouts.forget_title"),
            Some(t!("layouts.forget_hint")),
            |ui| ui.button(t!("layouts.forget_button")).clicked(),
        );
        if clicked {
            page.pending = Some(Pending::ForgetScreens);
        }
    });
}

/// Historique des instantanés, du plus récent au plus ancien.
fn history(m: &mut Manager, ui: &mut egui::Ui, page: &mut PageState) {
    let snapshots: Vec<SnapshotMeta> = page.index.snapshots.iter().rev().cloned().collect();
    widgets::section_title(ui, t!("layouts.history_title", count = snapshots.len()));
    ui.add_space(8.0);
    if snapshots.is_empty() {
        widgets::card(ui, |ui| {
            ui.label(widgets::dim(t!("layouts.history_empty")));
        });
        return;
    }
    let current = layouts::fingerprint(&m.desktop);
    widgets::card(ui, |ui| {
        for (i, s) in snapshots.iter().enumerate() {
            if i > 0 {
                widgets::row_separator(ui);
            }
            snapshot_row(m, ui, page, s, s.hash == current);
        }
    });
}

fn snapshot_row(
    m: &mut Manager,
    ui: &mut egui::Ui,
    page: &mut PageState,
    s: &SnapshotMeta,
    is_current: bool,
) {
    ui.horizontal(|ui| {
        let thumb = page.thumb(&m.store, s.id, s.hash).cloned();
        thumbnail(ui, thumb.as_ref(), THUMB_HEIGHT);
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon_button(ui, icons::DELETE, t!("common.delete")).clicked() {
                page.pending = Some(Pending::DeleteSnapshot(s.id));
            }
            if widgets::icon_button(ui, icons::EDIT, t!("common.rename")).clicked() {
                page.renaming = Some((s.id, snapshot_name(s)));
            }
            let restore = ui
                .add_enabled_ui(!is_current, |ui| {
                    ui.button(format!("{}  {}", icons::RESTORE, t!("layouts.restore")))
                })
                .inner;
            if restore.clicked() {
                page.pending = Some(Pending::Restore(s.id));
            }
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    let name = snapshot_name(s);
                    name_or_editor(m, ui, page, s.id, &name, "rename");
                    ui.horizontal_wrapped(|ui| {
                        let (fill, label) = match s.kind {
                            SnapshotKind::Auto => (theme::SURFACE_HI, t!("layouts.kind_auto")),
                            SnapshotKind::Manual => (theme::ACCENT, t!("layouts.kind_manual")),
                            SnapshotKind::First => (theme::ACCENT_2, t!("layouts.kind_first")),
                        };
                        let badge = widgets::badge(ui, fill, label);
                        if s.kind.protected() {
                            badge.on_hover_text(t!("layouts.protected_hint"));
                        }
                        if is_current {
                            widgets::badge(ui, theme::BORDER, t!("layouts.current"))
                                .on_hover_text(t!("layouts.current_hint"));
                        }
                        ui.label(widgets::dim(t!(
                            "layouts.snapshot_meta",
                            date = date(s.created),
                            count = s.boxes
                        )));
                    });
                });
            });
        });
    });
}

/// Nom affiché d'un instantané : le sien, sinon selon son type et sa raison.
fn snapshot_name(s: &SnapshotMeta) -> String {
    if !s.name.is_empty() {
        return s.name.clone();
    }
    match (s.kind, s.reason) {
        (SnapshotKind::First, _) => t!("layouts.name_first"),
        (_, SnapshotReason::BeforeRestore) => t!("layouts.name_before_restore"),
        (SnapshotKind::Manual, _) | (_, SnapshotReason::User) => t!("layouts.name_manual"),
        _ => t!("layouts.name_changed"),
    }
    .to_owned()
}

/// Nom en gras, ou champ de renommage (Entrée valide, Échap annule).
fn name_or_editor(
    m: &mut Manager,
    ui: &mut egui::Ui,
    page: &mut PageState,
    id: Uuid,
    name: &str,
    action: &str,
) {
    let Some((editing, draft)) = page.renaming.as_mut() else {
        ui.label(RichText::new(name).strong().size(theme::SIZE_M));
        return;
    };
    if *editing != id {
        ui.label(RichText::new(name).strong().size(theme::SIZE_M));
        return;
    }
    let response = ui.add(
        TextEdit::singleline(draft)
            .font(theme::strong(theme::SIZE_M))
            .desired_width(220.0),
    );
    if !response.has_focus() && !response.lost_focus() {
        response.request_focus();
    }
    let (enter, escape) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if escape {
        page.renaming = None;
    } else if response.lost_focus() || enter {
        let draft = draft.trim().to_owned();
        page.renaming = None;
        if !draft.is_empty()
            && draft != name
            && ask(m, page, action, json!({ "id": id, "name": draft }))
        {
            m.show_notice(t!("layouts.renamed"));
        }
    }
}

/// Miniature : l'écran principal et les cadres des boîtes (barre de titre marquée).
fn thumbnail(ui: &mut egui::Ui, thumb: Option<&Thumb>, height: f32) -> egui::Response {
    let ratio = thumb.map_or(16.0 / 9.0, |t| t.ratio).clamp(1.0, 3.6);
    let size = egui::vec2(height * ratio, height);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius::same(6),
        theme::BG,
        egui::Stroke::new(1.0, theme::BORDER),
        egui::StrokeKind::Inside,
    );
    let Some(thumb) = thumb else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icons::WARNING,
            egui::FontId::proportional(theme::SIZE_L),
            theme::TEXT_DIM,
        );
        return response.on_hover_text(t!("layouts.unreadable"));
    };
    let inner = rect.shrink(1.0);
    let painter = painter.with_clip_rect(inner);
    let bar = (height * 0.06).max(2.0);
    for b in &thumb.boxes {
        let min = inner.min + egui::vec2(b.rect.x * inner.width(), b.rect.y * inner.height());
        let full_height = if b.rolled {
            bar
        } else {
            b.rect.height * inner.height()
        };
        let frame = egui::Rect::from_min_size(
            min,
            egui::vec2(
                (b.rect.width * inner.width()).max(2.0),
                full_height.max(bar),
            ),
        );
        painter.rect_filled(frame, 2.0, theme::ACCENT.gamma_multiply(0.35));
        let title = egui::Rect::from_min_size(frame.min, egui::vec2(frame.width(), bar));
        painter.rect_filled(title, 2.0, theme::ACCENT);
    }
    response
}

/// Date lisible, dans le fuseau du système.
fn date(ts: Timestamp) -> String {
    ts.to_zoned_datetime(TimeZone::system())
        .strftime(t!("layouts.date_format"))
        .to_string()
}

/// Modales de confirmation et leurs actions.
fn confirmation(m: &mut Manager, ctx: &egui::Context, page: &mut PageState) {
    let Some(pending) = page.pending else {
        return;
    };
    let layout_name = |id: Uuid| {
        page.index
            .layout(id)
            .map(|l| l.name.clone())
            .unwrap_or_default()
    };
    let snapshot_label = |id: Uuid| {
        page.index
            .snapshot(id)
            .map(snapshot_name)
            .unwrap_or_default()
    };
    let answer = match pending {
        Pending::Restore(id) => widgets::confirm(
            ctx,
            "restaurer-instantané",
            &t!("layouts.restore_title", name = snapshot_label(id)),
            t!("layouts.restore_body"),
            t!("layouts.restore"),
            false,
        ),
        Pending::DeleteSnapshot(id) => {
            let protected = page.index.snapshot(id).is_some_and(|s| s.kind.protected());
            widgets::confirm(
                ctx,
                "supprimer-instantané",
                &t!("layouts.delete_title", name = snapshot_label(id)),
                if protected {
                    t!("layouts.delete_protected_body")
                } else {
                    t!("layouts.delete_body")
                },
                t!("common.delete"),
                true,
            )
        }
        Pending::ApplyLayout(id) => widgets::confirm(
            ctx,
            "appliquer-disposition",
            &t!("layouts.apply_title", name = layout_name(id)),
            t!("layouts.restore_body"),
            t!("layouts.apply"),
            false,
        ),
        Pending::UpdateLayout(id) => widgets::confirm(
            ctx,
            "remplacer-disposition",
            &t!("layouts.update_title", name = layout_name(id)),
            t!("layouts.update_body"),
            t!("layouts.update"),
            false,
        ),
        Pending::DeleteLayout(id) => widgets::confirm(
            ctx,
            "supprimer-disposition",
            &t!("layouts.delete_title", name = layout_name(id)),
            t!("layouts.delete_layout_body"),
            t!("common.delete"),
            true,
        ),
        Pending::ForgetScreens => widgets::confirm(
            ctx,
            "oublier-écrans",
            t!("layouts.forget_title"),
            t!("layouts.forget_body"),
            t!("layouts.forget_button"),
            false,
        ),
    };
    let Some(yes) = answer else {
        return;
    };
    page.pending = None;
    if !yes {
        return;
    }
    match pending {
        Pending::Restore(id) => {
            if ask(m, page, "restore", json!({ "id": id })) {
                m.show_notice(t!("layouts.restored"));
            }
        }
        Pending::DeleteSnapshot(id) => {
            if ask(m, page, "delete", json!({ "id": id })) {
                m.show_notice(t!("layouts.deleted"));
            }
        }
        Pending::ApplyLayout(id) => {
            if ask(m, page, "apply_layout", json!({ "id": id })) {
                m.show_notice(t!("layouts.applied"));
            }
        }
        Pending::UpdateLayout(id) => {
            if ask(m, page, "update_layout", json!({ "id": id })) {
                m.show_notice(t!("layouts.updated"));
            }
        }
        Pending::DeleteLayout(id) => {
            if ask(m, page, "delete_layout", json!({ "id": id })) {
                m.show_notice(t!("layouts.deleted"));
            }
        }
        Pending::ForgetScreens => forget_screens(m, &page.screens),
    }
}

/// Oublie, pour chaque boîte, les positions des autres configurations d'écrans.
fn forget_screens(m: &mut Manager, screens: &[Screen]) {
    if screens.is_empty() {
        return;
    }
    // Compte pour le message seulement : le bureau applique la commande à son état à
    // jour (une copie des boîtes, peut-être périmée, écraserait leurs changements).
    let count = m
        .desktop
        .boxes
        .iter()
        .filter(|b| {
            let mut placement = b.placement.clone();
            layouts::forget_other_screen_sets(&mut placement, screens)
        })
        .count();
    if !m.send(DesktopCommand::ForgetOtherScreenSets {
        screens: screens.to_vec(),
    }) {
        return;
    }
    m.show_notice(t!("layouts.forgotten", count = count));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use tidy_core::layouts::{Layouts, Taken};
    use tidy_core::{DeskBox, DesktopState, Settings};

    /// La page se dessine avec des instantanés et des dispositions, une modale ouverte
    /// et un renommage en cours, sans agent.
    #[test]
    fn page_draws_with_snapshots_and_layouts() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut state = DesktopState::default();
        state.boxes.push(DeskBox::new("Travail"));
        store.save(&state).unwrap();
        let layouts = Layouts::new(store.clone());
        let Taken::New(first) = layouts
            .take(
                &state,
                SnapshotKind::Auto,
                SnapshotReason::Changed,
                "",
                50,
                Timestamp::now(),
            )
            .unwrap()
        else {
            panic!("instantané attendu")
        };
        let layout = layouts
            .save_layout("Soir", &state, Timestamp::now())
            .unwrap();
        layouts
            .set_layout_hotkey(layout, Some("control+alt+Digit1".into()))
            .unwrap();

        let settings = Settings {
            onboarding_done: true,
            ..Settings::default()
        };
        let (_tx, rx) = mpsc::channel();
        let mut m = Manager::new(store, settings, rx, Some(super::super::Page::Layouts));
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let id = egui::Id::unique("page-dispositions");
        for step in 0..3 {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 700.0),
                )),
                ..Default::default()
            };
            let out = ctx.run_ui(raw, |ui| {
                if step == 1 {
                    ui.data_mut(|d| {
                        let page: &mut PageState = d.get_temp_mut_or_default(id);
                        page.renaming = Some((first.id, "Nouveau".into()));
                        page.pending = Some(Pending::DeleteSnapshot(first.id));
                    });
                }
                egui::CentralPanel::default().show(ui, |ui| show(&mut m, ui));
            });
            out.drop_without_applying_deltas();
        }
        let page: PageState = ctx.data_mut(|d| d.get_temp(id)).unwrap();
        assert_eq!(page.index.snapshots.len(), 1);
        assert_eq!(page.index.layouts.len(), 1);
        assert!(page.pending.is_some(), "la confirmation attend une réponse");
        // Un raccourci déjà pris par une disposition est refusé ailleurs.
        assert_eq!(
            hotkey_conflict(
                &m.settings,
                &m.desktop,
                &page.index,
                Uuid::new_v4(),
                "alt+control+Digit1"
            ),
            Some(t!("layouts.hotkey_owner", name = "Soir"))
        );
        assert_eq!(
            hotkey_conflict(
                &m.settings,
                &m.desktop,
                &page.index,
                layout,
                "control+alt+Digit1"
            ),
            None
        );
        assert!(
            hotkey_conflict(
                &m.settings,
                &m.desktop,
                &page.index,
                layout,
                "control+alt+KeyH"
            )
            .is_some(),
            "le raccourci du masquage"
        );
    }
}
