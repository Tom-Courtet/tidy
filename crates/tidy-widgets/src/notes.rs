//! iNotes : notes posées sur le bureau, une par widget.
//!
//! Le contenu d'une note vit dans son propre fichier, `<données>/notes/<id>.json`
//! (texte, couleur, dates), pas dans `desktop.json` : l'instance du widget ne garde que
//! l'identifiant de la note. Retirer le widget du bureau garde la note (« archivée »,
//! visible dans « Gérer les notes » de la fenêtre principale) ; elle n'est jamais
//! supprimée, seulement mise à la corbeille sur demande.
//!
//! Les écritures passent par un fil à part ([`queue_write`]), qui regroupe les frappes
//! rapprochées : l'interface ne touche pas au disque.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use egui::{Color32, CornerRadius, RichText, Ui};
use jiff::{Timestamp, ZonedDateTime};
use serde::{Deserialize, Serialize};
use tidy_ui::widgets::SWATCHES;
use tidy_ui::{icons, t, theme, widgets};
use uuid::Uuid;

use crate::{Widget, WidgetAction, WidgetCx, WidgetKind};

pub fn kind() -> WidgetKind {
    WidgetKind {
        id: "notes",
        name_key: "widgets.notes_name",
        icon: icons::NOTE,
        default_size: [240.0, 200.0],
        create: |v| {
            Box::new(NoteWidget::new(
                serde_json::from_value(v.clone()).unwrap_or_default(),
            ))
        },
    }
}

/// Couleurs des notes (indices dans [`SWATCHES`]) : jaune, lime, bleu ciel, rose,
/// violet, gris.
const COLOR_SWATCHES: [usize; 6] = [8, 5, 6, 11, 4, 1];

/// Couleur de fond d'une note d'après son indice.
pub fn note_color(index: u8) -> Color32 {
    let i = COLOR_SWATCHES
        .get(usize::from(index))
        .copied()
        .unwrap_or(COLOR_SWATCHES[0]);
    SWATCHES[i]
}

/// Une note, telle qu'enregistrée.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: Uuid,
    #[serde(default)]
    pub text: String,
    /// Indice de couleur (voir [`note_color`]).
    #[serde(default)]
    pub color: u8,
    pub created: Timestamp,
    pub updated: Timestamp,
}

impl Note {
    pub fn new(id: Uuid) -> Self {
        let now = Timestamp::now();
        Self {
            id,
            text: String::new(),
            color: 0,
            created: now,
            updated: now,
        }
    }

    /// Titre : première ligne non vide, coupée.
    pub fn title(&self) -> String {
        let line = self
            .text
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or_default();
        widgets::truncate(line, 48)
    }

    /// Le texte contient `query` (sans tenir compte de la casse).
    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        q.is_empty() || self.text.to_lowercase().contains(&q)
    }
}

/// Dossier des notes.
pub fn notes_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("notes")
}

pub fn note_path(data_dir: &Path, id: Uuid) -> PathBuf {
    notes_dir(data_dir).join(format!("{id}.json"))
}

/// Lit une note (`None` si elle n'existe pas ou est illisible).
pub fn load(data_dir: &Path, id: Uuid) -> Option<Note> {
    let bytes = std::fs::read(note_path(data_dir, id)).ok()?;
    serde_json::from_slice(&bytes)
        .inspect_err(|e| log::warn!("note {id} illisible : {e}"))
        .ok()
}

/// Enregistre une note tout de suite (écriture atomique).
pub fn save(data_dir: &Path, note: &Note) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(note).map_err(|e| e.to_string())?;
    let dir = notes_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    tidy_core::store::write_atomic(&note_path(data_dir, note.id), &bytes)
        .map_err(|e| format!("{e:#}"))
}

/// Toutes les notes du dossier, la plus récemment modifiée d'abord.
pub fn list(data_dir: &Path) -> Vec<Note> {
    let Ok(entries) = std::fs::read_dir(notes_dir(data_dir)) else {
        return Vec::new();
    };
    let mut notes: Vec<Note> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "json") {
                return None;
            }
            let bytes = std::fs::read(&path).ok()?;
            serde_json::from_slice(&bytes).ok()
        })
        .collect();
    notes.sort_by_key(|n| std::cmp::Reverse(n.updated));
    notes
}

/// Enregistre une note sur le fil des écritures : les écritures rapprochées d'une même
/// note n'en font qu'une, environ 250 ms plus tard.
pub fn queue_write(data_dir: &Path, note: &Note) {
    static WRITER: OnceLock<Option<Sender<(PathBuf, Note)>>> = OnceLock::new();
    let writer = WRITER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<(PathBuf, Note)>();
        std::thread::Builder::new()
            .name("tidy-notes".into())
            .spawn(move || {
                while let Ok(first) = rx.recv() {
                    let mut pending: HashMap<Uuid, (PathBuf, Note)> = HashMap::new();
                    pending.insert(first.1.id, first);
                    std::thread::sleep(Duration::from_millis(250));
                    while let Ok(next) = rx.try_recv() {
                        pending.insert(next.1.id, next);
                    }
                    for (data_dir, note) in pending.into_values() {
                        if let Err(e) = save(&data_dir, &note) {
                            log::error!("note {} non enregistrée : {e}", note.id);
                        }
                    }
                }
            })
            .inspect_err(|e| log::error!("fil des notes : {e}"))
            .ok()?;
        Some(tx)
    });
    let sent = writer
        .as_ref()
        .is_some_and(|tx| tx.send((data_dir.to_path_buf(), note.clone())).is_ok());
    if !sent && let Err(e) = save(data_dir, note) {
        log::error!("note {} non enregistrée : {e}", note.id);
    }
}

/// Regroupement par date de « Gérer les notes ».
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DateGroup {
    Today,
    Yesterday,
    ThisWeek,
    ThisMonth,
    Older,
}

impl DateGroup {
    pub fn label(self) -> &'static str {
        match self {
            DateGroup::Today => t!("widgets.notes_today"),
            DateGroup::Yesterday => t!("widgets.notes_yesterday"),
            DateGroup::ThisWeek => t!("widgets.notes_this_week"),
            DateGroup::ThisMonth => t!("widgets.notes_this_month"),
            DateGroup::Older => t!("widgets.notes_older"),
        }
    }

    /// Groupe d'une date de modification, vue depuis `now` (dans le fuseau de `now`).
    /// La semaine commence le lundi.
    pub fn of(when: Timestamp, now: &ZonedDateTime) -> DateGroup {
        let day = when.to_zoned_datetime(now.time_zone().clone()).date();
        let today = now.date();
        if day >= today {
            return DateGroup::Today;
        }
        if today.yesterday().is_ok_and(|y| day == y) {
            return DateGroup::Yesterday;
        }
        let monday_offset = i64::from(today.weekday().to_monday_zero_offset());
        let week_start = today.checked_sub(jiff::Span::new().days(monday_offset));
        if week_start.is_ok_and(|w| day >= w) {
            return DateGroup::ThisWeek;
        }
        if day.year() == today.year() && day.month() == today.month() {
            return DateGroup::ThisMonth;
        }
        DateGroup::Older
    }
}

/// Réglages du widget : seulement l'identifiant de sa note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteSettings {
    pub note: Uuid,
}

impl Default for NoteSettings {
    fn default() -> Self {
        Self {
            note: Uuid::new_v4(),
        }
    }
}

pub struct NoteWidget {
    s: NoteSettings,
    /// Note lue au premier affichage.
    note: Option<Note>,
}

impl NoteWidget {
    pub fn new(s: NoteSettings) -> Self {
        Self { s, note: None }
    }

    /// Lit la note au premier affichage, ou la crée.
    fn note(&mut self, data_dir: &Path) -> &mut Note {
        let id = self.s.note;
        self.note.get_or_insert_with(|| {
            load(data_dir, id).unwrap_or_else(|| {
                let note = Note::new(id);
                queue_write(data_dir, &note);
                note
            })
        })
    }
}

impl Widget for NoteWidget {
    fn ui(&mut self, ui: &mut Ui, cx: &mut WidgetCx) {
        let data_dir = cx.data_dir.clone();
        let id = egui::Id::unique(("tidy-note", self.s.note));
        let note = self.note(&data_dir);
        let rect = ui.max_rect();
        let fill = note_color(note.color);
        let ink = theme::on(fill);
        ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
        let inner = rect.shrink(8.0);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        child.visuals_mut().extreme_bg_color = fill;
        child.visuals_mut().selection.bg_fill = ink.gamma_multiply(0.25);
        child.visuals_mut().text_cursor.stroke.color = ink;
        let changed = egui::ScrollArea::vertical()
            .id_salt(id.with("défilement"))
            .auto_shrink(false)
            .show(&mut child, |ui| {
                ui.add_sized(
                    ui.available_size(),
                    egui::TextEdit::multiline(&mut note.text)
                        .id(id)
                        .frame(egui::Frame::NONE)
                        .text_color(ink)
                        .font(egui::FontId::proportional(theme::SIZE_M))
                        .hint_text(
                            RichText::new(t!("widgets.notes_hint")).color(ink.gamma_multiply(0.5)),
                        ),
                )
                .changed()
            })
            .inner;
        if changed {
            note.updated = Timestamp::now();
            let note = note.clone();
            queue_write(&data_dir, &note);
        }
    }

    fn settings_ui(&mut self, ui: &mut Ui, cx: &mut WidgetCx) -> bool {
        let data_dir = cx.data_dir.clone();
        let note = self.note(&data_dir);
        ui.label(t!("widgets.notes_color"));
        let mut picked = None;
        ui.horizontal_wrapped(|ui| {
            for i in 0..COLOR_SWATCHES.len() as u8 {
                if widgets::swatch(ui, note_color(i), note.color == i).clicked() {
                    picked = Some(i);
                }
            }
        });
        if let Some(i) = picked
            && i != note.color
        {
            note.color = i;
            note.updated = Timestamp::now();
            let note = note.clone();
            queue_write(&data_dir, &note);
        }
        ui.add_space(8.0);
        widgets::hint(ui, t!("widgets.notes_file_hint"));
        if ui
            .button(format!("{}  {}", icons::LIST, t!("widgets.notes_manage")))
            .clicked()
        {
            cx.actions.push(WidgetAction::OpenManager("widgets".into()));
        }
        // La couleur est dans le fichier de la note : les réglages de l'instance ne
        // changent pas.
        false
    }

    fn save(&self) -> serde_json::Value {
        serde_json::to_value(&self.s).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_saved_listed_and_searched() {
        let dir = tempfile::tempdir().unwrap();
        assert!(list(dir.path()).is_empty());
        let mut a = Note::new(Uuid::new_v4());
        a.text = "\n  Courses  \nlait, pain".into();
        save(dir.path(), &a).unwrap();
        let mut b = Note::new(Uuid::new_v4());
        b.text = "Appeler Paul".into();
        b.updated = a
            .updated
            .checked_add(jiff::SignedDuration::from_secs(60))
            .unwrap();
        save(dir.path(), &b).unwrap();
        // Un fichier étranger est ignoré.
        std::fs::write(notes_dir(dir.path()).join("autre.txt"), "x").unwrap();
        let all = list(dir.path());
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, b.id, "la plus récente d'abord");
        assert_eq!(load(dir.path(), a.id).unwrap(), a);
        assert_eq!(a.title(), "Courses");
        assert!(a.matches("PAIN") && a.matches("") && !a.matches("paul"));
        assert_eq!(Note::new(Uuid::new_v4()).title(), "");
    }

    #[test]
    fn queued_writes_land_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut note = Note::new(Uuid::new_v4());
        for i in 0..5 {
            note.text = format!("version {i}");
            queue_write(dir.path(), &note);
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if load(dir.path(), note.id).is_some_and(|n| n.text == "version 4") {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "note pas écrite");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn widget_keeps_its_note_id() {
        let w = NoteWidget::new(NoteSettings::default());
        let saved = w.save();
        let again: NoteSettings = serde_json::from_value(saved.clone()).unwrap();
        assert_eq!(again, w.s);
        // Deux nouvelles notes n'ont pas le même identifiant.
        assert_ne!(NoteSettings::default(), NoteSettings::default());
    }

    #[test]
    fn date_groups() {
        let tz = jiff::tz::TimeZone::get("Europe/Paris").unwrap();
        // Jeudi 26 novembre 2026, 15 h.
        let now: ZonedDateTime = "2026-11-26T15:00:00[Europe/Paris]".parse().unwrap();
        let at = |s: &str| -> Timestamp {
            let z: ZonedDateTime = format!("{s}[Europe/Paris]").parse().unwrap();
            z.with_time_zone(tz.clone()).timestamp()
        };
        assert_eq!(
            DateGroup::of(at("2026-11-26T08:00:00"), &now),
            DateGroup::Today
        );
        assert_eq!(
            DateGroup::of(at("2026-11-25T23:59:00"), &now),
            DateGroup::Yesterday
        );
        // Lundi de la même semaine.
        assert_eq!(
            DateGroup::of(at("2026-11-23T09:00:00"), &now),
            DateGroup::ThisWeek
        );
        assert_eq!(
            DateGroup::of(at("2026-11-02T09:00:00"), &now),
            DateGroup::ThisMonth
        );
        assert_eq!(
            DateGroup::of(at("2026-10-31T09:00:00"), &now),
            DateGroup::Older
        );
        assert!(DateGroup::Today < DateGroup::Older);
    }

    #[test]
    fn colors_fall_back_to_yellow() {
        assert_eq!(note_color(0), SWATCHES[8]);
        assert_eq!(note_color(200), SWATCHES[8]);
    }
}
