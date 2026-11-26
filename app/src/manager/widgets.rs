//! Page « Widgets » (lot H) : catalogue des types de widgets (« Ajouter » pose le
//! widget à une place libre de l'écran principal), widgets posés sur le bureau
//! (geler, exclure du masquage, supprimer), réglages communs et « Gérer les notes »
//! (recherche, regroupement par date, notes archivées).
//!
//! Comme le reste de la fenêtre, la page n'écrit jamais `desktop.json` : elle envoie
//! des [`DesktopCommand`] au bureau. Les fichiers des notes, eux, sont lus ici ; une
//! note n'est créée ici qu'au moment de « Nouvelle note », et n'est jamais supprimée :
//! « Mettre à la corbeille » passe par la corbeille du système, après confirmation.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use egui::{CornerRadius, Key, Modifiers, RichText};
use serde_json::Value;
use tidy_core::DesktopCommand;
use tidy_core::model::WidgetInstance;
use tidy_ui::{icons, t, theme, widgets};
use tidy_widgets::notes::{self, DateGroup, Note};
use uuid::Uuid;

use super::Manager;
use super::pages::{card_title, page_header};

/// Relecture du dossier des notes (une note change sur le bureau pendant qu'on lit).
const NOTES_EVERY: Duration = Duration::from_secs(2);

/// Action qui attend une confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    RemoveWidget(Uuid),
    TrashNote(Uuid),
}

/// État de la page, gardé dans la mémoire d'egui.
#[derive(Clone, Default)]
struct PageState {
    query: String,
    notes: Vec<Note>,
    read_at: Option<Instant>,
    pending: Option<Pending>,
}

fn state_id() -> egui::Id {
    egui::Id::unique("tidy-page-widgets")
}

pub fn show(m: &mut Manager, ui: &mut egui::Ui) {
    let mut st: PageState = ui
        .ctx()
        .data(|d| d.get_temp(state_id()).unwrap_or_default());
    if st.read_at.is_none_or(|t| t.elapsed() >= NOTES_EVERY) {
        st.notes = notes::list(m.store.root());
        st.read_at = Some(Instant::now());
    }
    ui.ctx().request_repaint_after(NOTES_EVERY);

    page_header(
        ui,
        icons::WIDGETS,
        t!("manager.widgets"),
        t!("widgets.page_intro"),
    );
    catalogue(m, ui, &mut st);
    ui.add_space(12.0);
    placed(m, ui, &mut st);
    ui.add_space(12.0);
    common(m, ui);
    ui.add_space(12.0);
    notes_manager(m, ui, &mut st);
    confirmation(m, ui.ctx(), &mut st);
    ui.ctx().data_mut(|d| d.insert_temp(state_id(), st));
}

/// Nom affiché d'un type de widget.
fn kind_name(kind: &str) -> (&'static str, String) {
    match tidy_widgets::kind(kind) {
        Some(k) => (k.icon, tidy_ui::i18n::tr(k.name_key).to_owned()),
        None => (icons::WIDGETS, kind.to_owned()),
    }
}

/// Note d'une instance du widget « notes ».
fn note_of(w: &WidgetInstance) -> Option<Uuid> {
    (w.kind == "notes")
        .then(|| w.settings.get("note")?.as_str()?.parse().ok())
        .flatten()
}

fn new_instance(kind: &str, settings: Value) -> WidgetInstance {
    WidgetInstance {
        id: Uuid::new_v4(),
        kind: kind.to_owned(),
        settings,
        placement: Default::default(),
        frozen: false,
        exclude_from_quick_hide: false,
    }
}

/// Pose un widget sur le bureau (place libre choisie par le bureau).
fn add(m: &mut Manager, st: &mut PageState, kind: &str) {
    let settings = if kind == "notes" {
        match new_note(m, st) {
            Some(id) => serde_json::json!({ "note": id }),
            None => return,
        }
    } else {
        Value::Null
    };
    if m.send(DesktopCommand::UpsertWidget {
        w: new_instance(kind, settings),
    }) {
        m.show_notice(t!("widgets.added", name = kind_name(kind).1));
    }
}

/// Crée le fichier d'une nouvelle note.
fn new_note(m: &mut Manager, st: &mut PageState) -> Option<Uuid> {
    let note = Note::new(Uuid::new_v4());
    match notes::save(m.store.root(), &note) {
        Ok(()) => {
            st.read_at = None;
            Some(note.id)
        }
        Err(e) => {
            m.error = Some(e);
            None
        }
    }
}

fn catalogue(m: &mut Manager, ui: &mut egui::Ui, st: &mut PageState) {
    widgets::card(ui, |ui| {
        card_title(ui, icons::PLUS, t!("widgets.catalogue"));
        widgets::hint(ui, t!("widgets.catalogue_hint"));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            for k in tidy_widgets::kinds() {
                let text = RichText::new(format!(
                    "{}  {}   {}",
                    k.icon,
                    tidy_ui::i18n::tr(k.name_key),
                    icons::PLUS
                ))
                .size(theme::SIZE_M);
                if ui
                    .add(egui::Button::new(text).min_size(egui::vec2(0.0, 34.0)))
                    .on_hover_text(t!("widgets.add"))
                    .clicked()
                {
                    add(m, st, k.id);
                }
            }
        });
    });
}

fn placed(m: &mut Manager, ui: &mut egui::Ui, st: &mut PageState) {
    let list = m.desktop.widgets.clone();
    widgets::card(ui, |ui| {
        card_title(ui, icons::MONITOR, t!("widgets.placed"));
        if list.is_empty() {
            widgets::hint(ui, t!("widgets.placed_none"));
            return;
        }
        for (i, w) in list.iter().enumerate() {
            if i > 0 {
                widgets::row_separator(ui);
            }
            let (icon, mut name) = kind_name(&w.kind);
            if let Some(note) = note_of(w).and_then(|id| st.notes.iter().find(|n| n.id == id)) {
                let title = note.title();
                if !title.is_empty() {
                    name = format!("{name} · {title}");
                }
            }
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(icon)
                        .size(theme::SIZE_L)
                        .color(theme::ACCENT_2),
                );
                ui.label(name);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::icon_button(ui, icons::DELETE, t!("widgets.delete")).clicked() {
                        st.pending = Some(Pending::RemoveWidget(w.id));
                    }
                    let mut exclude = w.exclude_from_quick_hide;
                    if widgets::choice(ui, exclude, t!("widgets.exclude_hide_short"))
                        .on_hover_text(t!("widgets.exclude_hide_hint"))
                        .clicked()
                    {
                        exclude = !exclude;
                        // Seulement le drapeau : les réglages et la place du widget,
                        // peut-être changés sur le bureau depuis, restent ceux du bureau.
                        m.send(DesktopCommand::SetWidgetFlags {
                            id: w.id,
                            frozen: None,
                            exclude_from_quick_hide: Some(exclude),
                        });
                    }
                    if widgets::choice(ui, w.frozen, t!("widgets.freeze"))
                        .on_hover_text(t!("widgets.freeze_hint"))
                        .clicked()
                    {
                        m.send(DesktopCommand::SetWidgetFlags {
                            id: w.id,
                            frozen: Some(!w.frozen),
                            exclude_from_quick_hide: None,
                        });
                    }
                });
            });
        }
    });
}

fn common(m: &mut Manager, ui: &mut egui::Ui) {
    widgets::card(ui, |ui| {
        card_title(ui, icons::SETTINGS, t!("widgets.common"));
        let s = &mut m.settings.widgets;
        let mut changed = false;
        changed |= widgets::setting_row(
            ui,
            icons::EYE_OFF,
            t!("widgets.hide_with_desktop"),
            Some(t!("widgets.hide_with_desktop_hint")),
            |ui| widgets::toggle(ui, &mut s.hide_with_desktop).changed(),
        );
        widgets::row_separator(ui);
        changed |= widgets::setting_row(
            ui,
            icons::BELL,
            t!("widgets.notification_sound"),
            Some(t!("widgets.notification_sound_hint")),
            |ui| widgets::toggle(ui, &mut s.notification_sound).changed(),
        );
        widgets::row_separator(ui);
        changed |= widgets::setting_row(
            ui,
            icons::GRIP,
            t!("widgets.frame_on_hover"),
            Some(t!("widgets.frame_on_hover_hint")),
            |ui| widgets::toggle(ui, &mut s.frame_on_hover).changed(),
        );
        if changed {
            m.save();
        }
    });
}

/// Date de modification d'une note (« 26 nov. 14:05 »).
fn note_date(note: &Note) -> String {
    let z = note.updated.to_zoned_datetime(jiff::tz::TimeZone::system());
    format!(
        "{} {} {:02}:{:02}",
        z.day(),
        tidy_widgets::clock::month_short(z.month()),
        z.hour(),
        z.minute()
    )
}

fn notes_manager(m: &mut Manager, ui: &mut egui::Ui, st: &mut PageState) {
    let on_desktop: HashSet<Uuid> = m.desktop.widgets.iter().filter_map(note_of).collect();
    widgets::card(ui, |ui| {
        card_title(ui, icons::NOTE, t!("widgets.notes_manage"));
        widgets::hint(ui, t!("widgets.notes_manage_hint"));
        ui.add_space(6.0);
        let search_id = egui::Id::unique("tidy-recherche-notes");
        ui.horizontal(|ui| {
            let search = ui.add(
                egui::TextEdit::singleline(&mut st.query)
                    .id(search_id)
                    .hint_text(format!("{}  {}", icons::SEARCH, t!("widgets.notes_search")))
                    .desired_width(260.0),
            );
            // Ctrl+F (⌘F) : la recherche.
            if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F)) {
                search.request_focus();
            }
            if widgets::primary(ui, format!("{}  {}", icons::PLUS, t!("widgets.notes_new")))
                .clicked()
            {
                add(m, st, "notes");
            }
            if widgets::icon_button(ui, icons::REFRESH, t!("widgets.notes_refresh")).clicked() {
                st.read_at = None;
            }
        });
        ui.add_space(8.0);
        let now = jiff::Timestamp::now().to_zoned_datetime(jiff::tz::TimeZone::system());
        let found: Vec<&Note> = st.notes.iter().filter(|n| n.matches(&st.query)).collect();
        if found.is_empty() {
            widgets::hint(
                ui,
                if st.notes.is_empty() {
                    t!("widgets.notes_none")
                } else {
                    t!("widgets.notes_no_match")
                },
            );
            return;
        }
        let mut action: Option<(Uuid, NoteAction)> = None;
        let mut group = None;
        for note in found {
            let g = DateGroup::of(note.updated, &now);
            if group != Some(g) {
                group = Some(g);
                ui.add_space(6.0);
                ui.label(
                    RichText::new(g.label())
                        .color(theme::TEXT_DIM)
                        .size(theme::SIZE_S),
                );
            }
            let placed = on_desktop.contains(&note.id);
            if let Some(a) = note_row(ui, note, placed) {
                action = Some((note.id, a));
            }
        }
        if let Some((id, a)) = action {
            match a {
                NoteAction::Show => {
                    if m.send(DesktopCommand::UpsertWidget {
                        w: new_instance("notes", serde_json::json!({ "note": id })),
                    }) {
                        m.show_notice(t!("widgets.notes_shown"));
                    }
                }
                NoteAction::Hide => {
                    let ids: Vec<Uuid> = m
                        .desktop
                        .widgets
                        .iter()
                        .filter(|w| note_of(w) == Some(id))
                        .map(|w| w.id)
                        .collect();
                    for w in ids {
                        m.send(DesktopCommand::RemoveWidget { id: w });
                    }
                }
                NoteAction::Trash => st.pending = Some(Pending::TrashNote(id)),
            }
        }
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoteAction {
    /// Remettre une note archivée sur le bureau.
    Show,
    /// Retirer du bureau (la note reste).
    Hide,
    Trash,
}

fn note_row(ui: &mut egui::Ui, note: &Note, placed: bool) -> Option<NoteAction> {
    let mut action = None;
    egui::Frame::new()
        .fill(theme::SURFACE_HI)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 32.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(r, CornerRadius::same(3), notes::note_color(note.color));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let title = note.title();
                    if title.is_empty() {
                        ui.label(widgets::dim(t!("widgets.notes_empty")));
                    } else {
                        ui.label(RichText::new(title).size(theme::SIZE_M));
                    }
                    let preview: String = note
                        .text
                        .lines()
                        .map(str::trim)
                        .filter(|l| !l.is_empty())
                        .skip(1)
                        .take(2)
                        .collect::<Vec<_>>()
                        .join(" · ");
                    let status = if placed {
                        t!("widgets.notes_on_desktop")
                    } else {
                        t!("widgets.notes_archived")
                    };
                    let mut line = format!("{} · {status}", note_date(note));
                    if !preview.is_empty() {
                        line = format!("{line} · {}", widgets::truncate(&preview, 60));
                    }
                    ui.label(widgets::dim(line));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if placed {
                        if ui.button(t!("widgets.notes_hide")).clicked() {
                            action = Some(NoteAction::Hide);
                        }
                    } else {
                        if widgets::icon_button(ui, icons::DELETE, t!("widgets.notes_trash"))
                            .clicked()
                        {
                            action = Some(NoteAction::Trash);
                        }
                        if ui.button(t!("widgets.notes_show")).clicked() {
                            action = Some(NoteAction::Show);
                        }
                    }
                });
            });
        });
    ui.add_space(4.0);
    action
}

fn confirmation(m: &mut Manager, ctx: &egui::Context, st: &mut PageState) {
    let Some(pending) = st.pending else {
        return;
    };
    let (title, body, label) = match pending {
        Pending::RemoveWidget(id) => {
            let note = m
                .desktop
                .widgets
                .iter()
                .find(|w| w.id == id)
                .is_some_and(|w| w.kind == "notes");
            (
                t!("widgets.delete_title"),
                if note {
                    t!("widgets.delete_note_body")
                } else {
                    t!("widgets.delete_body")
                },
                t!("common.delete"),
            )
        }
        Pending::TrashNote(_) => (
            t!("widgets.notes_trash_title"),
            t!("widgets.notes_trash_body"),
            t!("widgets.notes_trash"),
        ),
    };
    match widgets::confirm(ctx, "tidy-confirmer-widget", title, body, label, true) {
        Some(true) => {
            st.pending = None;
            match pending {
                Pending::RemoveWidget(id) => {
                    m.send(DesktopCommand::RemoveWidget { id });
                }
                Pending::TrashNote(id) => {
                    let path = notes::note_path(m.store.root(), id);
                    match tidy_platform::current().trash(&[path]) {
                        Ok(()) => m.show_notice(t!("widgets.notes_trashed")),
                        Err(e) => m.error = Some(format!("{e:#}")),
                    }
                    st.read_at = None;
                }
            }
        }
        Some(false) => st.pending = None,
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_widgets_point_to_their_note() {
        let id = Uuid::new_v4();
        let w = new_instance("notes", serde_json::json!({ "note": id }));
        assert_eq!(note_of(&w), Some(id));
        assert_eq!(note_of(&new_instance("clock", Value::Null)), None);
        assert_eq!(note_of(&new_instance("notes", Value::Null)), None);
    }
}
