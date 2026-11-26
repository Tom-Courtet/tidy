//! Widget Météo : une ou plusieurs villes, temps actuel, prochaines heures et jours
//! suivants (Open-Meteo, sans clé). Actualisation toutes les 30 minutes ; une erreur
//! réseau garde les dernières données et propose de réessayer.
//!
//! Aucun appel réseau avant le premier affichage du widget posé sur le bureau.

use std::collections::HashMap;
use std::f32::consts::PI;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Color32, Painter, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};
use tidy_integrations::NetError;
use tidy_integrations::task::{Polled, Task};
use tidy_integrations::weather::{
    Condition, Forecast, OpenMeteo, Place, TempUnit, WeatherProvider,
};
use tidy_ui::theme::Typo;
use tidy_ui::{icons, t, theme, widgets};

use crate::online::{self, error_notice, net_error_text};
use crate::{Widget, WidgetCx};

/// Intervalle entre deux actualisations.
const REFRESH: Duration = Duration::from_secs(30 * 60);
/// Délai avant de réessayer seul après une erreur.
const RETRY: Duration = Duration::from_secs(5 * 60);

/// Réglages enregistrés.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeatherSettings {
    pub places: Vec<Place>,
    pub unit: TempUnit,
    /// Ville affichée en détail (indice dans `places`).
    pub current: usize,
    /// Bandeau des prochaines heures.
    pub show_hours: bool,
    /// Nombre de jours de prévision (1 à 7).
    pub days: u8,
}

impl Default for WeatherSettings {
    fn default() -> Self {
        Self {
            places: Vec::new(),
            unit: TempUnit::Celsius,
            current: 0,
            show_hours: true,
            days: 5,
        }
    }
}

/// État d'une ville.
#[derive(Default)]
struct Slot {
    forecast: Option<Forecast>,
    /// Dernière tentative (réussie ou non).
    tried: Option<Instant>,
    error: Option<NetError>,
    task: Option<Task<Result<Forecast, NetError>>>,
}

impl Slot {
    fn due(&self, now: Instant) -> Option<Duration> {
        if self.task.is_some() {
            return None;
        }
        let wait = if self.error.is_some() { RETRY } else { REFRESH };
        Some(match self.tried {
            Some(t) => (t + wait).saturating_duration_since(now),
            None => Duration::ZERO,
        })
    }
}

/// Recherche d'une ville (réglages).
#[derive(Default)]
struct Search {
    query: String,
    task: Option<Task<Result<Vec<Place>, NetError>>>,
    results: Option<Result<Vec<Place>, NetError>>,
}

pub struct WeatherWidget {
    s: WeatherSettings,
    provider: Arc<dyn WeatherProvider>,
    slots: HashMap<String, Slot>,
    search: Search,
}

/// Création depuis les réglages enregistrés (aucun réseau ici).
pub fn create(settings: &serde_json::Value) -> Box<dyn Widget> {
    Box::new(WeatherWidget::new(
        serde_json::from_value(settings.clone()).unwrap_or_default(),
        Arc::new(OpenMeteo),
    ))
}

/// Clé d'une ville dans `slots` (les coordonnées et l'unité changent la prévision).
fn slot_key(p: &Place, unit: TempUnit) -> String {
    format!("{:.4},{:.4},{unit:?}", p.latitude, p.longitude)
}

impl WeatherWidget {
    fn new(s: WeatherSettings, provider: Arc<dyn WeatherProvider>) -> Self {
        Self {
            s,
            provider,
            slots: HashMap::new(),
            search: Search::default(),
        }
    }

    /// Relève les tâches finies et lance les actualisations dues.
    fn pump(&mut self, ui: &Ui) {
        let now = Instant::now();
        let unit = self.s.unit;
        let keys: Vec<(String, Place)> = self
            .s
            .places
            .iter()
            .map(|p| (slot_key(p, unit), p.clone()))
            .collect();
        self.slots
            .retain(|k, _| keys.iter().any(|(key, _)| key == k));
        for (key, place) in keys {
            let slot = self.slots.entry(key).or_default();
            if let Some(task) = &slot.task {
                match task.poll() {
                    Polled::Pending => continue,
                    Polled::Done(Ok(f)) => {
                        slot.forecast = Some(f);
                        slot.error = None;
                    }
                    Polled::Done(Err(e)) => {
                        log::warn!("météo de {} : {e}", place.name);
                        slot.error = Some(e);
                    }
                    Polled::Lost => slot.error = Some(NetError::Other("fil perdu".into())),
                }
                slot.task = None;
                slot.tried = Some(now);
            }
            if slot.due(now) == Some(Duration::ZERO) {
                self::spawn(slot, &self.provider, place, unit, ui);
            }
        }
        if let Some(task) = &self.search.task {
            match task.poll() {
                Polled::Pending => {}
                Polled::Done(r) => {
                    self.search.results = Some(r);
                    self.search.task = None;
                }
                Polled::Lost => {
                    self.search.results = Some(Err(NetError::Other("fil perdu".into())));
                    self.search.task = None;
                }
            }
        }
    }

    fn current_place(&self) -> Option<&Place> {
        self.s
            .places
            .get(self.s.current)
            .or_else(|| self.s.places.first())
    }
}

fn spawn(
    slot: &mut Slot,
    provider: &Arc<dyn WeatherProvider>,
    place: Place,
    unit: TempUnit,
    ui: &Ui,
) {
    let provider = provider.clone();
    slot.task = Some(Task::spawn("meteo", online::waker(ui), move || {
        provider.forecast(&place, unit)
    }));
}

/// « 8° » (arrondi, unité courte).
fn deg(v: f64) -> String {
    format!("{}°", v.round() as i64)
}

impl Widget for WeatherWidget {
    fn ui(&mut self, ui: &mut Ui, cx: &mut WidgetCx) {
        self.pump(ui);
        let Some(place) = self.current_place().cloned() else {
            ui.vertical_centered(|ui| {
                ui.add_space(12.0);
                ui.label(
                    RichText::new(icons::WEATHER)
                        .size(32.0)
                        .color(theme::ACCENT),
                );
                ui.add_space(4.0);
                widgets::hint(ui, t!("widgets2.weather_empty"));
            });
            return;
        };
        let key = slot_key(&place, self.s.unit);

        // En-tête : ville (et passage d'une ville à l'autre), actualiser.
        ui.horizontal(|ui| {
            let many = self.s.places.len() > 1;
            if many
                && widgets::icon_button(ui, icons::CHEVRON_LEFT, t!("widgets2.weather_prev"))
                    .clicked()
            {
                let n = self.s.places.len();
                self.s.current = (self.s.current.min(n - 1) + n - 1) % n;
                cx.changed = true;
            }
            ui.label(
                RichText::new(widgets::truncate(&place.name, 22))
                    .bold()
                    .size(theme::SIZE_L),
            )
            .on_hover_text(place.label());
            if many
                && widgets::icon_button(ui, icons::CHEVRON_RIGHT, t!("widgets2.weather_next"))
                    .clicked()
            {
                self.s.current =
                    (self.s.current.min(self.s.places.len() - 1) + 1) % self.s.places.len();
                cx.changed = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let busy = self.slots.get(&key).is_some_and(|s| s.task.is_some());
                if busy {
                    ui.add(egui::Spinner::new().size(14.0));
                } else if widgets::icon_button(ui, icons::REFRESH, t!("widgets2.refresh")).clicked()
                    && let Some(slot) = self.slots.get_mut(&key)
                {
                    self::spawn(slot, &self.provider, place.clone(), self.s.unit, ui);
                }
            });
        });

        let slot = self.slots.get(&key);
        let error = slot.and_then(|s| s.error.clone());
        let Some(f) = slot.and_then(|s| s.forecast.clone()) else {
            match error {
                Some(e) => {
                    if error_notice(ui, &net_error_text(&e), true)
                        && let Some(slot) = self.slots.get_mut(&key)
                    {
                        self::spawn(slot, &self.provider, place.clone(), self.s.unit, ui);
                    }
                }
                None => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                        ui.label(widgets::dim(t!("widgets2.loading")));
                    });
                }
            }
            return;
        };

        // Temps actuel.
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
            paint_condition(ui.painter(), rect, f.current.condition, f.current.is_day);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new(deg(f.current.temperature)).title().size(30.0));
                ui.label(condition_text(f.current.condition));
            });
        });
        let mut details = vec![t!("widgets2.feels_like", temp = deg(f.current.apparent))];
        if let Some(h) = f.current.humidity {
            details.push(t!("widgets2.humidity", value = h.round() as i64));
        }
        if let Some(w) = f.current.wind {
            details.push(t!(
                "widgets2.wind",
                value = w.round() as i64,
                unit = f.unit.wind_symbol()
            ));
        }
        widgets::hint(ui, details.join(" · "));

        // Prochaines heures (toutes les deux heures, défilement horizontal).
        if self.s.show_hours && !f.hours.is_empty() {
            ui.add_space(2.0);
            egui::ScrollArea::horizontal()
                .id_salt("heures")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for h in f.hours.iter().step_by(2).take(12) {
                            ui.vertical(|ui| {
                                ui.set_width(34.0);
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(widgets::dim(format!("{}h", h.time.hour())));
                                let (rect, _) =
                                    ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
                                paint_condition(ui.painter(), rect, h.condition, h.is_day);
                                ui.label(RichText::new(deg(h.temperature)).size(theme::SIZE_S));
                            })
                            .response
                            .on_hover_text(condition_text(h.condition));
                        }
                    });
                });
        }

        // Jours suivants : nom, icône, min, barre, max.
        let days: Vec<_> = f
            .days
            .iter()
            .take(usize::from(self.s.days.clamp(1, 7)))
            .collect();
        if !days.is_empty() {
            widgets::row_separator(ui);
            let lo = days.iter().map(|d| d.min).fold(f64::INFINITY, f64::min);
            let hi = days.iter().map(|d| d.max).fold(f64::NEG_INFINITY, f64::max);
            for (i, d) in days.iter().enumerate() {
                ui.horizontal(|ui| {
                    let name = if i == 0 {
                        t!("widgets2.today").to_owned()
                    } else {
                        online::weekday_short(d.date.weekday()).to_owned()
                    };
                    // Colonne alignée à gauche, assez large pour « Aujourd'hui ».
                    ui.allocate_ui_with_layout(
                        vec2(80.0, 18.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_width(80.0);
                            ui.add(egui::Label::new(name).truncate());
                        },
                    );
                    let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                    paint_condition(ui.painter(), rect, d.condition, true);
                    let rain = d
                        .precipitation
                        .filter(|p| *p >= 20.0)
                        .map(|p| format!("{}%", p.round() as i64))
                        .unwrap_or_default();
                    ui.add_sized([30.0, 18.0], egui::Label::new(widgets::dim(rain)));
                    ui.add_sized([28.0, 18.0], egui::Label::new(widgets::dim(deg(d.min))));
                    let width = (ui.available_width() - 34.0).max(20.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(width, 6.0), Sense::hover());
                    paint_range(ui.painter(), rect, lo, hi, d.min, d.max);
                    ui.label(deg(d.max));
                })
                .response
                .on_hover_text(condition_text(d.condition));
            }
        }

        if let Some(e) = &error {
            // Données anciennes gardées ; l'erreur reste discrète.
            online::footer(ui, format!("{}  {}", icons::WARNING, net_error_text(e)));
        } else {
            online::footer(
                ui,
                t!("widgets2.weather_footer", time = online::hm(f.current.time)),
            );
        }
    }

    fn settings_ui(&mut self, ui: &mut Ui, cx: &mut WidgetCx) -> bool {
        self.pump(ui);
        let mut changed = false;
        widgets::section_title(ui, t!("widgets2.weather_places"));
        let mut remove = None;
        let mut make_current = None;
        for (i, p) in self.s.places.iter().enumerate() {
            ui.horizontal(|ui| {
                if widgets::choice(ui, i == self.s.current, widgets::truncate(&p.label(), 34))
                    .on_hover_text(t!("widgets2.weather_show"))
                    .clicked()
                {
                    make_current = Some(i);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::icon_button(ui, icons::DELETE, t!("widgets2.remove")).clicked() {
                        remove = Some(i);
                    }
                });
            });
        }
        if let Some(i) = make_current {
            self.s.current = i;
            changed = true;
        }
        if let Some(i) = remove {
            self.s.places.remove(i);
            if self.s.current >= self.s.places.len() {
                self.s.current = self.s.places.len().saturating_sub(1);
            }
            changed = true;
        }

        // Recherche d'une ville par son nom (pas de géolocalisation).
        ui.horizontal(|ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.search.query)
                    .hint_text(t!("widgets2.weather_search_hint"))
                    .desired_width(180.0),
            );
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let searching = self.search.task.is_some();
            if searching {
                ui.add(egui::Spinner::new());
            } else if (ui
                .button(format!("{}  {}", icons::SEARCH, t!("widgets2.search")))
                .clicked()
                || enter)
                && self.search.query.trim().chars().count() >= 2
            {
                let provider = self.provider.clone();
                let query = self.search.query.trim().to_owned();
                let lang = match tidy_ui::i18n::language() {
                    tidy_ui::Lang::Fr => "fr",
                    tidy_ui::Lang::En => "en",
                };
                self.search.results = None;
                self.search.task = Some(Task::spawn(
                    "meteo-recherche",
                    online::waker(ui),
                    move || provider.search(&query, lang),
                ));
            }
        });
        let mut add = None;
        match &self.search.results {
            Some(Ok(found)) if found.is_empty() => {
                widgets::hint(ui, t!("widgets2.weather_none"));
            }
            Some(Ok(found)) => {
                for p in found {
                    let known =
                        self.s.places.iter().any(|q| {
                            slot_key(q, TempUnit::Celsius) == slot_key(p, TempUnit::Celsius)
                        });
                    ui.add_enabled_ui(!known, |ui| {
                        if ui
                            .button(format!("{}  {}", icons::PLUS, p.label()))
                            .clicked()
                        {
                            add = Some(p.clone());
                        }
                    });
                }
            }
            Some(Err(e)) => {
                widgets::hint(ui, net_error_text(e));
            }
            None => {}
        }
        if let Some(p) = add {
            self.s.places.push(p);
            self.s.current = self.s.places.len() - 1;
            self.search = Search::default();
            changed = true;
        }

        widgets::row_separator(ui);
        widgets::setting_row(ui, "", t!("widgets2.unit"), None, |ui| {
            for unit in [TempUnit::Fahrenheit, TempUnit::Celsius] {
                if widgets::choice(ui, self.s.unit == unit, unit.symbol()).clicked()
                    && self.s.unit != unit
                {
                    self.s.unit = unit;
                    changed = true;
                }
            }
        });
        widgets::setting_row(ui, "", t!("widgets2.weather_hours"), None, |ui| {
            if widgets::toggle(ui, &mut self.s.show_hours).changed() {
                changed = true;
            }
        });
        widgets::setting_row(ui, "", t!("widgets2.weather_days"), None, |ui| {
            if ui.add(egui::Slider::new(&mut self.s.days, 1..=7)).changed() {
                changed = true;
            }
        });
        widgets::hint(ui, t!("widgets2.weather_source"));
        if changed {
            cx.changed = true;
        }
        changed
    }

    fn save(&self) -> serde_json::Value {
        serde_json::to_value(&self.s).unwrap_or_default()
    }

    fn repaint_after(&self) -> Option<Duration> {
        let now = Instant::now();
        self.slots
            .values()
            .filter_map(|s| s.due(now))
            .min()
            .map(|d| d.max(Duration::from_secs(1)))
    }
}

/// Texte d'un temps (« Nuageux »).
fn condition_text(c: Condition) -> &'static str {
    match c {
        Condition::Clear => t!("widgets2.cond_clear"),
        Condition::PartlyCloudy => t!("widgets2.cond_partly_cloudy"),
        Condition::Cloudy => t!("widgets2.cond_cloudy"),
        Condition::Fog => t!("widgets2.cond_fog"),
        Condition::Drizzle => t!("widgets2.cond_drizzle"),
        Condition::Rain => t!("widgets2.cond_rain"),
        Condition::FreezingRain => t!("widgets2.cond_freezing_rain"),
        Condition::Snow => t!("widgets2.cond_snow"),
        Condition::Showers => t!("widgets2.cond_showers"),
        Condition::SnowShowers => t!("widgets2.cond_snow_showers"),
        Condition::Thunderstorm => t!("widgets2.cond_thunderstorm"),
        Condition::Unknown => t!("widgets2.cond_unknown"),
    }
}

/// Barre des températures d'un jour sur l'échelle de la semaine.
fn paint_range(painter: &Painter, rect: Rect, lo: f64, hi: f64, min: f64, max: f64) {
    painter.rect_filled(rect, 3, theme::SURFACE_HI);
    let span = (hi - lo).max(1.0);
    let x = |v: f64| rect.left() + rect.width() * ((v - lo) / span) as f32;
    let bar = Rect::from_x_y_ranges(x(min)..=x(max).max(x(min) + 4.0), rect.y_range());
    painter.rect_filled(bar, 3, theme::ACCENT_2);
}

/// Pictogramme d'un temps, dessiné (la police d'icônes n'a qu'un nuage et un soleil).
pub(crate) fn paint_condition(painter: &Painter, rect: Rect, c: Condition, day: bool) {
    let s = rect.width().min(rect.height());
    let c0 = rect.center();
    let u = s / 24.0;
    let sky = |painter: &Painter, at: Pos2, r: f32| {
        if day {
            paint_sun(painter, at, r);
        } else {
            paint_moon(painter, at, r);
        }
    };
    match c {
        Condition::Clear => sky(painter, c0, 6.0 * u),
        Condition::PartlyCloudy | Condition::Showers | Condition::SnowShowers => {
            sky(painter, c0 + vec2(-4.0 * u, -4.0 * u), 4.5 * u);
            paint_cloud(
                painter,
                c0 + vec2(2.0 * u, 2.0 * u),
                16.0 * u,
                theme::TEXT_DIM,
            );
            if c == Condition::Showers {
                paint_rain(painter, c0 + vec2(2.0 * u, 8.0 * u), u, 2);
            } else if c == Condition::SnowShowers {
                paint_snow(painter, c0 + vec2(2.0 * u, 8.5 * u), u, 2);
            }
        }
        Condition::Cloudy | Condition::Unknown => {
            paint_cloud(painter, c0, 20.0 * u, theme::TEXT_DIM)
        }
        Condition::Fog => {
            paint_cloud(painter, c0 + vec2(0.0, -3.0 * u), 18.0 * u, theme::TEXT_DIM);
            for k in 0..3 {
                let y = c0.y + (4.0 + 2.5 * k as f32) * u;
                painter.line_segment(
                    [pos2(c0.x - 8.0 * u, y), pos2(c0.x + 8.0 * u, y)],
                    Stroke::new(1.5 * u, theme::TEXT_DIM),
                );
            }
        }
        Condition::Drizzle | Condition::Rain | Condition::FreezingRain => {
            paint_cloud(painter, c0 + vec2(0.0, -3.0 * u), 18.0 * u, theme::TEXT_DIM);
            let n = if c == Condition::Drizzle { 2 } else { 3 };
            paint_rain(painter, c0 + vec2(0.0, 5.0 * u), u, n);
            if c == Condition::FreezingRain {
                paint_snow(painter, c0 + vec2(5.0 * u, 9.0 * u), u, 1);
            }
        }
        Condition::Snow => {
            paint_cloud(painter, c0 + vec2(0.0, -3.0 * u), 18.0 * u, theme::TEXT_DIM);
            paint_snow(painter, c0 + vec2(0.0, 6.0 * u), u, 3);
        }
        Condition::Thunderstorm => {
            paint_cloud(painter, c0 + vec2(0.0, -3.0 * u), 18.0 * u, theme::TEXT_DIM);
            let pts = [
                c0 + vec2(1.0 * u, 2.0 * u),
                c0 + vec2(-2.0 * u, 7.0 * u),
                c0 + vec2(1.0 * u, 7.0 * u),
                c0 + vec2(-1.5 * u, 11.0 * u),
            ];
            painter.add(egui::Shape::line(
                pts.to_vec(),
                Stroke::new(1.8 * u, theme::ACCENT_2),
            ));
        }
    }
}

fn paint_sun(painter: &Painter, c: Pos2, r: f32) {
    painter.circle_filled(c, r * 0.62, theme::WARNING);
    for k in 0..8 {
        let a = k as f32 * PI / 4.0;
        let dir = Vec2::angled(a);
        painter.line_segment(
            [c + dir * r * 0.85, c + dir * r * 1.15],
            Stroke::new((r * 0.16).max(1.0), theme::WARNING),
        );
    }
}

/// Croissant plein : bande entre deux arcs (maillage, la forme n'est pas convexe).
fn paint_moon(painter: &Painter, c: Pos2, r: f32) {
    let mut mesh = egui::Mesh::default();
    let color: Color32 = theme::TEXT_DIM;
    let n = 24;
    // Arc extérieur de 60° à 300° (côté gauche), arc intérieur d'un cercle décalé.
    let d = 0.6 * r;
    let inner_r = (0.76f32).sqrt() * r;
    let a0 = PI / 3.0;
    let a1 = 5.0 * PI / 3.0;
    let b0 = (0.866f32).atan2(-0.1);
    let b1 = 2.0 * PI - b0;
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let a = a0 + (a1 - a0) * t;
        let b = b0 + (b1 - b0) * t;
        mesh.colored_vertex(c + Vec2::angled(a) * r, color);
        mesh.colored_vertex(c + vec2(d, 0.0) + Vec2::angled(b) * inner_r, color);
    }
    for i in 0..n {
        let k = 2 * i as u32;
        mesh.add_triangle(k, k + 1, k + 2);
        mesh.add_triangle(k + 1, k + 3, k + 2);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Nuage plein de largeur `w`, centré sur `c`.
fn paint_cloud(painter: &Painter, c: Pos2, w: f32, color: Color32) {
    let h = w * 0.36;
    let base = Rect::from_center_size(c + vec2(0.0, h * 0.35), vec2(w, h));
    painter.rect_filled(base, h / 2.0, color);
    painter.circle_filled(c + vec2(-w * 0.2, 0.0), w * 0.22, color);
    painter.circle_filled(c + vec2(w * 0.1, -w * 0.08), w * 0.3, color);
}

fn paint_rain(painter: &Painter, top: Pos2, u: f32, n: usize) {
    for k in 0..n {
        let x = top.x + (k as f32 - (n as f32 - 1.0) / 2.0) * 4.0 * u;
        painter.line_segment(
            [pos2(x + 1.0 * u, top.y), pos2(x - 1.0 * u, top.y + 4.0 * u)],
            Stroke::new(1.6 * u, theme::ACCENT),
        );
    }
}

fn paint_snow(painter: &Painter, top: Pos2, u: f32, n: usize) {
    for k in 0..n {
        let x = top.x + (k as f32 - (n as f32 - 1.0) / 2.0) * 4.5 * u;
        painter.circle_filled(pos2(x, top.y + 1.5 * u), 1.4 * u, theme::TEXT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing::render;
    use std::sync::Mutex;
    use tidy_integrations::weather::{parse_forecast, parse_places};

    /// Fournisseur sans réseau : rend les réponses enregistrées.
    struct Recorded {
        calls: Mutex<usize>,
        fail: bool,
    }

    impl WeatherProvider for Recorded {
        fn search(&self, _: &str, _: &str) -> Result<Vec<Place>, NetError> {
            parse_places(include_str!(
                "../../tidy-integrations/tests/data/open-meteo-geocoding.json"
            ))
        }

        fn forecast(&self, _: &Place, unit: TempUnit) -> Result<Forecast, NetError> {
            *self.calls.lock().unwrap() += 1;
            if self.fail {
                return Err(NetError::Offline);
            }
            parse_forecast(
                include_str!("../../tidy-integrations/tests/data/open-meteo-forecast.json"),
                unit,
            )
        }
    }

    fn lyon() -> Place {
        parse_places(include_str!(
            "../../tidy-integrations/tests/data/open-meteo-geocoding.json"
        ))
        .unwrap()
        .remove(0)
    }

    fn wait(w: &mut WeatherWidget) {
        for _ in 0..200 {
            if w.slots.values().all(|s| s.task.is_none()) && !w.slots.is_empty() {
                break;
            }
            render(w, 1);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn empty_widget_does_not_touch_the_network() {
        let provider = Arc::new(Recorded {
            calls: Mutex::new(0),
            fail: false,
        });
        let mut w = WeatherWidget::new(WeatherSettings::default(), provider.clone());
        render(&mut w, 2);
        assert_eq!(*provider.calls.lock().unwrap(), 0);
        assert_eq!(w.repaint_after(), None);
    }

    #[test]
    fn forecast_is_fetched_once_then_every_half_hour() {
        let provider = Arc::new(Recorded {
            calls: Mutex::new(0),
            fail: false,
        });
        let s = WeatherSettings {
            places: vec![lyon()],
            ..Default::default()
        };
        let mut w = WeatherWidget::new(s, provider.clone());
        render(&mut w, 1);
        wait(&mut w);
        render(&mut w, 3);
        assert_eq!(*provider.calls.lock().unwrap(), 1);
        let slot = w.slots.values().next().unwrap();
        assert!(slot.forecast.is_some() && slot.error.is_none());
        let next = w.repaint_after().unwrap();
        assert!(next > Duration::from_secs(29 * 60) && next <= REFRESH);
    }

    #[test]
    fn network_error_is_shown_and_retried_later() {
        let provider = Arc::new(Recorded {
            calls: Mutex::new(0),
            fail: true,
        });
        let s = WeatherSettings {
            places: vec![lyon()],
            ..Default::default()
        };
        let mut w = WeatherWidget::new(s, provider.clone());
        render(&mut w, 1);
        wait(&mut w);
        render(&mut w, 1);
        let slot = w.slots.values().next().unwrap();
        assert_eq!(slot.error, Some(NetError::Offline));
        assert!(w.repaint_after().unwrap() <= RETRY);
    }

    #[test]
    fn settings_survive_a_round_trip() {
        let s = WeatherSettings {
            places: vec![lyon()],
            unit: TempUnit::Fahrenheit,
            current: 0,
            show_hours: false,
            days: 3,
        };
        let w = WeatherWidget::new(s.clone(), Arc::new(OpenMeteo));
        let again: WeatherSettings = serde_json::from_value(w.save()).unwrap();
        assert_eq!(again, s);
    }

    #[test]
    fn every_condition_has_a_picture() {
        let ctx = egui::Context::default();
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            for code in [0, 2, 3, 45, 51, 61, 66, 71, 80, 85, 95, 7] {
                for day in [true, false] {
                    let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
                    paint_condition(ui.painter(), rect, Condition::from_wmo(code), day);
                }
            }
        });
        crate::online::testing::discard(output);
    }
}
