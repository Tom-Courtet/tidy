//! Horloge : numérique ou analogique, heure locale et autres fuseaux (base IANA de
//! `jiff`), trois tailles, date affichée ou non.
//!
//! Sans les secondes, l'horloge ne se redessine qu'au changement de minute : au repos,
//! elle ne coûte presque rien.

use std::time::Duration;

use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Stroke, Ui, pos2, vec2};
use jiff::Timestamp;
use jiff::civil::Weekday;
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};
use tidy_ui::{icons, t, theme, widgets};

use crate::{Widget, WidgetCx, WidgetKind};

pub fn kind() -> WidgetKind {
    WidgetKind {
        id: "clock",
        name_key: "widgets.clock_name",
        icon: icons::CLOCK,
        default_size: [200.0, 110.0],
        create: |v| {
            Box::new(Clock::new(
                serde_json::from_value(v.clone()).unwrap_or_default(),
            ))
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockStyle {
    #[default]
    Digital,
    Analog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl ClockSize {
    fn scale(self) -> f32 {
        match self {
            ClockSize::Small => 0.75,
            ClockSize::Medium => 1.0,
            ClockSize::Large => 1.45,
        }
    }
}

/// Réglages d'une horloge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClockSettings {
    pub style: ClockStyle,
    pub size: ClockSize,
    /// Heure locale en premier.
    pub show_local: bool,
    /// Autres fuseaux, noms IANA (`Asia/Tokyo`…).
    pub zones: Vec<String>,
    pub show_date: bool,
    pub seconds: bool,
    pub h24: bool,
}

impl Default for ClockSettings {
    fn default() -> Self {
        Self {
            style: ClockStyle::Digital,
            size: ClockSize::Medium,
            show_local: true,
            zones: Vec::new(),
            show_date: true,
            seconds: false,
            h24: true,
        }
    }
}

/// Une horloge affichée : libellé et fuseau.
struct Face {
    label: String,
    tz: TimeZone,
}

pub struct Clock {
    s: ClockSettings,
    faces: Vec<Face>,
    /// Recherche d'un fuseau à ajouter (réglages).
    query: String,
    /// Noms de la base des fuseaux, lus à la première recherche.
    all_zones: Option<Vec<String>>,
}

impl Clock {
    pub fn new(s: ClockSettings) -> Self {
        let mut clock = Self {
            s,
            faces: Vec::new(),
            query: String::new(),
            all_zones: None,
        };
        clock.resolve();
        clock
    }

    /// Fuseaux des réglages → fuseaux affichés (un nom inconnu est ignoré).
    fn resolve(&mut self) {
        self.faces = faces(&self.s);
    }
}

fn faces(s: &ClockSettings) -> Vec<Face> {
    let mut out = Vec::new();
    if s.show_local || s.zones.is_empty() {
        out.push(Face {
            label: t!("widgets.clock_local").to_owned(),
            tz: TimeZone::system(),
        });
    }
    for name in &s.zones {
        match TimeZone::get(name) {
            Ok(tz) => out.push(Face {
                label: zone_label(name),
                tz,
            }),
            Err(e) => log::debug!("fuseau {name} inconnu : {e}"),
        }
    }
    out
}

/// Nom lisible d'un fuseau : `America/New_York` → « New York ».
pub fn zone_label(name: &str) -> String {
    name.rsplit('/').next().unwrap_or(name).replace('_', " ")
}

/// Fuseaux dont le nom contient `query` (sans tenir compte de la casse ni des `_`),
/// au plus `max`, hors de `exclude`.
pub fn search_zones<'a>(
    all: &'a [String],
    query: &str,
    exclude: &[String],
    max: usize,
) -> Vec<&'a str> {
    let q = query.trim().to_lowercase().replace('_', " ");
    if q.is_empty() {
        return Vec::new();
    }
    all.iter()
        .filter(|n| !exclude.contains(n))
        .filter(|n| n.to_lowercase().replace('_', " ").contains(&q))
        .take(max)
        .map(String::as_str)
        .collect()
}

/// Délai jusqu'au prochain changement de seconde (ou de minute).
pub fn until_next_tick(now: Timestamp, seconds: bool) -> Duration {
    let period: i64 = if seconds { 1_000 } else { 60_000 };
    let ms = now.as_millisecond().rem_euclid(period);
    // Un peu après la limite, pour ne pas redessiner juste avant.
    Duration::from_millis((period - ms) as u64 + 15)
}

fn weekday_short(d: Weekday) -> &'static str {
    match d {
        Weekday::Monday => t!("widgets.day_mon"),
        Weekday::Tuesday => t!("widgets.day_tue"),
        Weekday::Wednesday => t!("widgets.day_wed"),
        Weekday::Thursday => t!("widgets.day_thu"),
        Weekday::Friday => t!("widgets.day_fri"),
        Weekday::Saturday => t!("widgets.day_sat"),
        Weekday::Sunday => t!("widgets.day_sun"),
    }
}

/// Nom court d'un mois (1 à 12).
pub fn month_short(m: i8) -> &'static str {
    match m {
        1 => t!("widgets.month_1"),
        2 => t!("widgets.month_2"),
        3 => t!("widgets.month_3"),
        4 => t!("widgets.month_4"),
        5 => t!("widgets.month_5"),
        6 => t!("widgets.month_6"),
        7 => t!("widgets.month_7"),
        8 => t!("widgets.month_8"),
        9 => t!("widgets.month_9"),
        10 => t!("widgets.month_10"),
        11 => t!("widgets.month_11"),
        _ => t!("widgets.month_12"),
    }
}

/// Heure affichée (`14:05`, `2:05 PM`, avec ou sans secondes).
pub fn format_time(hour: i8, minute: i8, second: i8, h24: bool, seconds: bool) -> String {
    let (h, suffix) = if h24 {
        (hour, "")
    } else {
        let h12 = match hour % 12 {
            0 => 12,
            h => h,
        };
        (h12, if hour < 12 { " AM" } else { " PM" })
    };
    if seconds {
        format!("{h:02}:{minute:02}:{second:02}{suffix}")
    } else {
        format!("{h:02}:{minute:02}{suffix}")
    }
}

/// Hauteurs (en points, à l'échelle 1) des lignes d'une horloge numérique.
const LABEL_H: f32 = 16.0;
const TIME_H: f32 = 40.0;
const DATE_H: f32 = 16.0;
const DIAL: f32 = 88.0;

impl Clock {
    fn digital_width(&self) -> f32 {
        let base = if self.s.seconds { 180.0 } else { 140.0 };
        let base = if self.s.h24 { base } else { base + 40.0 };
        base * self.s.size.scale()
    }

    fn row_height(&self) -> f32 {
        let date = if self.s.show_date { DATE_H } else { 0.0 };
        (LABEL_H + TIME_H + date) * self.s.size.scale()
    }

    fn digital(&self, ui: &mut Ui, rect: Rect) {
        let s = self.s.size.scale();
        let now = Timestamp::now();
        let rows = self.faces.len().max(1) as f32;
        let row_h = self.row_height();
        let gap = 6.0 * s;
        let total = rows * row_h + (rows - 1.0) * gap;
        let mut y = rect.center().y - total / 2.0;
        let painter = ui.painter();
        for face in &self.faces {
            let z = now.to_zoned_datetime(face.tz.clone());
            let x = rect.center().x;
            painter.text(
                pos2(x, y),
                Align2::CENTER_TOP,
                &face.label,
                FontId::proportional(theme::SIZE_S * s),
                theme::TEXT_DIM,
            );
            painter.text(
                pos2(x, y + LABEL_H * s),
                Align2::CENTER_TOP,
                format_time(z.hour(), z.minute(), z.second(), self.s.h24, self.s.seconds),
                theme::display(30.0 * s),
                theme::TEXT,
            );
            if self.s.show_date {
                painter.text(
                    pos2(x, y + (LABEL_H + TIME_H) * s),
                    Align2::CENTER_TOP,
                    date_text(&z),
                    FontId::proportional(theme::SIZE_S * s),
                    theme::ACCENT_2,
                );
            }
            y += row_h + gap;
        }
    }

    fn analog(&self, ui: &mut Ui, rect: Rect) {
        let s = self.s.size.scale();
        let now = Timestamp::now();
        let n = self.faces.len().max(1) as f32;
        let cell_w = (DIAL + 16.0) * s;
        let left = rect.center().x - n * cell_w / 2.0;
        let date = if self.s.show_date { DATE_H } else { 0.0 };
        let total_h = (DIAL + LABEL_H + 4.0 + date) * s;
        let top = rect.center().y - total_h / 2.0;
        let painter = ui.painter();
        for (i, face) in self.faces.iter().enumerate() {
            let z = now.to_zoned_datetime(face.tz.clone());
            let cx = left + cell_w * (i as f32 + 0.5);
            let center = pos2(cx, top + DIAL * s / 2.0);
            dial(
                painter,
                center,
                DIAL * s / 2.0,
                z.hour(),
                z.minute(),
                z.second(),
                self.s.seconds,
            );
            let below = top + DIAL * s + 4.0 * s;
            painter.text(
                pos2(cx, below),
                Align2::CENTER_TOP,
                &face.label,
                FontId::proportional(theme::SIZE_S * s),
                theme::TEXT_DIM,
            );
            if self.s.show_date {
                painter.text(
                    pos2(cx, below + LABEL_H * s),
                    Align2::CENTER_TOP,
                    date_text(&z),
                    FontId::proportional(theme::SIZE_XS * s),
                    theme::ACCENT_2,
                );
            }
        }
    }
}

fn date_text(z: &jiff::ZonedDateTime) -> String {
    format!(
        "{} {} {}",
        weekday_short(z.weekday()),
        z.day(),
        month_short(z.month())
    )
}

/// Cadran : graduations, aiguilles des heures et des minutes, trotteuse en option.
fn dial(painter: &egui::Painter, c: Pos2, r: f32, hour: i8, minute: i8, second: i8, seconds: bool) {
    painter.circle_filled(c, r, theme::SURFACE_HI);
    painter.circle_stroke(c, r, Stroke::new(1.5, theme::BORDER));
    for i in 0..12 {
        let a = i as f32 / 12.0 * std::f32::consts::TAU;
        let dir = vec2(a.sin(), -a.cos());
        let (inner, width, color) = if i % 3 == 0 {
            (0.78, 2.0, theme::TEXT)
        } else {
            (0.86, 1.0, theme::TEXT_DIM)
        };
        painter.line_segment(
            [c + dir * r * inner, c + dir * r * 0.94],
            Stroke::new(width, color),
        );
    }
    let hand = |turns: f32, len: f32, width: f32, color: Color32| {
        let a = turns * std::f32::consts::TAU;
        let dir = vec2(a.sin(), -a.cos());
        painter.line_segment(
            [c - dir * r * 0.08, c + dir * r * len],
            Stroke::new(width, color),
        );
    };
    let m = f32::from(minute) + f32::from(second) / 60.0;
    let h = f32::from(hour % 12) + m / 60.0;
    hand(h / 12.0, 0.5, 3.0, theme::TEXT);
    hand(m / 60.0, 0.75, 2.0, theme::TEXT);
    if seconds {
        hand(f32::from(second) / 60.0, 0.82, 1.0, theme::ACCENT_2);
    }
    painter.circle_filled(c, 3.0, theme::ACCENT);
}

impl Widget for Clock {
    fn ui(&mut self, ui: &mut Ui, _cx: &mut WidgetCx) {
        let rect = ui.max_rect();
        match self.s.style {
            ClockStyle::Digital => self.digital(ui, rect),
            ClockStyle::Analog => self.analog(ui, rect),
        }
    }

    fn settings_ui(&mut self, ui: &mut Ui, _cx: &mut WidgetCx) -> bool {
        let mut changed = false;
        let s = &mut self.s;
        ui.horizontal_wrapped(|ui| {
            ui.label(t!("widgets.clock_style"));
            changed |= widgets::choice_value(
                ui,
                &mut s.style,
                ClockStyle::Digital,
                t!("widgets.clock_digital"),
            )
            .changed();
            changed |= widgets::choice_value(
                ui,
                &mut s.style,
                ClockStyle::Analog,
                t!("widgets.clock_analog"),
            )
            .changed();
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(t!("widgets.size"));
            for (size, label) in [
                (ClockSize::Small, t!("widgets.size_small")),
                (ClockSize::Medium, t!("widgets.size_medium")),
                (ClockSize::Large, t!("widgets.size_large")),
            ] {
                changed |= widgets::choice_value(ui, &mut s.size, size, label).changed();
            }
        });
        ui.add_space(4.0);
        for (value, label) in [
            (&mut s.show_date, t!("widgets.clock_show_date")),
            (&mut s.seconds, t!("widgets.clock_seconds")),
            (&mut s.h24, t!("widgets.clock_24h")),
            (&mut s.show_local, t!("widgets.clock_show_local")),
        ] {
            changed |= widgets::setting_row(ui, "", label, None, |ui| {
                widgets::toggle(ui, value).changed()
            });
        }
        ui.add_space(6.0);
        widgets::section_title(ui, t!("widgets.clock_zones"));
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            for (i, name) in s.zones.iter().enumerate() {
                if widgets::chip(ui, &zone_label(name)) {
                    remove = Some(i);
                }
            }
        });
        if let Some(i) = remove {
            s.zones.remove(i);
            changed = true;
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .hint_text(t!("widgets.clock_zone_hint"))
                .desired_width(f32::INFINITY),
        );
        if !self.query.trim().is_empty() {
            let all = self.all_zones.get_or_insert_with(|| {
                let mut names: Vec<String> = jiff::tz::db()
                    .available()
                    .map(|n| n.as_str().to_owned())
                    .collect();
                names.sort();
                names
            });
            let found = search_zones(all, &self.query, &s.zones, 8);
            if found.is_empty() {
                ui.label(widgets::dim(t!("widgets.clock_zone_none")));
            }
            let mut added = None;
            for name in found {
                let text = RichText::new(format!("{}  {name}", icons::PLUS));
                if ui.selectable_label(false, text).clicked() {
                    added = Some(name.to_owned());
                }
            }
            if let Some(name) = added {
                s.zones.push(name);
                self.query.clear();
                changed = true;
            }
        }
        if changed {
            self.resolve();
        }
        changed
    }

    fn save(&self) -> serde_json::Value {
        serde_json::to_value(&self.s).unwrap_or_default()
    }

    fn repaint_after(&self) -> Option<Duration> {
        Some(until_next_tick(Timestamp::now(), self.s.seconds))
    }

    fn preferred_size(&self) -> Option<[f32; 2]> {
        let s = self.s.size.scale();
        let n = self.faces.len().max(1) as f32;
        Some(match self.s.style {
            ClockStyle::Digital => [
                self.digital_width() + 24.0 * s,
                n * self.row_height() + (n - 1.0) * 6.0 * s + 16.0 * s,
            ],
            ClockStyle::Analog => {
                let date = if self.s.show_date { DATE_H } else { 0.0 };
                [
                    n * (DIAL + 16.0) * s + 16.0 * s,
                    (DIAL + LABEL_H + 4.0 + date) * s + 16.0 * s,
                ]
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_of_zones() {
        assert_eq!(zone_label("America/New_York"), "New York");
        assert_eq!(zone_label("Europe/Paris"), "Paris");
        assert_eq!(zone_label("UTC"), "UTC");
        assert_eq!(zone_label("America/Argentina/Buenos_Aires"), "Buenos Aires");
    }

    #[test]
    fn zones_are_resolved_and_unknown_ones_ignored() {
        let s = ClockSettings {
            show_local: false,
            zones: vec!["Asia/Tokyo".into(), "Pas/Un_Fuseau".into(), "UTC".into()],
            ..Default::default()
        };
        let f = faces(&s);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].label, "Tokyo");
        // Tokyo : UTC+9, sans heure d'été.
        let ts: Timestamp = "2026-11-26T12:00:00Z".parse().unwrap();
        assert_eq!(ts.to_zoned_datetime(f[0].tz.clone()).hour(), 21);
        assert_eq!(ts.to_zoned_datetime(f[1].tz.clone()).hour(), 12);
        // Aucun fuseau : l'heure locale, même si elle est désactivée.
        let none = ClockSettings {
            show_local: false,
            ..Default::default()
        };
        assert_eq!(faces(&none).len(), 1);
    }

    #[test]
    fn zone_search() {
        let all: Vec<String> = ["America/New_York", "Asia/Tokyo", "Europe/Paris"]
            .map(String::from)
            .to_vec();
        assert_eq!(search_zones(&all, "new york", &[], 8), ["America/New_York"]);
        assert_eq!(search_zones(&all, "TOK", &[], 8), ["Asia/Tokyo"]);
        assert!(search_zones(&all, "  ", &[], 8).is_empty());
        assert!(search_zones(&all, "paris", &["Europe/Paris".into()], 8).is_empty());
        assert_eq!(search_zones(&all, "a", &[], 2).len(), 2);
        // La vraie base contient les grandes villes.
        let db: Vec<String> = jiff::tz::db()
            .available()
            .map(|n| n.as_str().to_owned())
            .collect();
        assert!(
            db.iter().any(|n| n == "Europe/Paris"),
            "base des fuseaux vide"
        );
    }

    #[test]
    fn time_formats() {
        assert_eq!(format_time(14, 5, 9, true, false), "14:05");
        assert_eq!(format_time(14, 5, 9, true, true), "14:05:09");
        assert_eq!(format_time(0, 30, 0, false, false), "12:30 AM");
        assert_eq!(format_time(12, 0, 0, false, false), "12:00 PM");
        assert_eq!(format_time(23, 59, 59, false, true), "11:59:59 PM");
    }

    #[test]
    fn repaints_at_the_next_minute_or_second() {
        let ts: Timestamp = "2026-11-26T12:00:59.500Z".parse().unwrap();
        let d = until_next_tick(ts, false);
        assert!(d >= Duration::from_millis(500) && d < Duration::from_millis(600));
        let d = until_next_tick(ts, true);
        assert!(d >= Duration::from_millis(500) && d < Duration::from_millis(600));
        let ts: Timestamp = "2026-11-26T12:00:00Z".parse().unwrap();
        assert!(until_next_tick(ts, false) > Duration::from_secs(59));
    }

    #[test]
    fn settings_round_trip_and_size() {
        let s = ClockSettings {
            style: ClockStyle::Analog,
            size: ClockSize::Large,
            zones: vec!["Asia/Tokyo".into()],
            seconds: true,
            ..Default::default()
        };
        let c = Clock::new(s.clone());
        let back: ClockSettings = serde_json::from_value(c.save()).unwrap();
        assert_eq!(back, s);
        let [w, h] = c.preferred_size().unwrap();
        let small = Clock::new(ClockSettings::default())
            .preferred_size()
            .unwrap();
        assert!(w > small[0] && h > small[1], "deux grands cadrans");
    }
}
