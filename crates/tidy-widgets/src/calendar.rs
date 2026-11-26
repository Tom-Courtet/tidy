//! Widget Calendrier : vue du mois ou agenda des prochains jours, événements d'un ou
//! plusieurs agendas ICS (Google, Outlook…), chacun avec sa couleur, et rappels
//! (`WidgetAction::Notify`) quelques minutes avant le début.
//!
//! Les agendas se rechargent toutes les 15 minutes, sur un fil à part. Aucun appel
//! réseau avant le premier affichage du widget posé sur le bureau.
//!
//! L'adresse ICS d'un agenda est secrète (chez Google, elle donne accès à son contenu) :
//! elle est rangée dans le gestionnaire d'identifiants du système (« Tidy » /
//! « ics-<identifiant de l'agenda> »), jamais dans les réglages du widget (donc ni dans
//! `desktop.json`, ni dans les instantanés, ni sur le socket). Les réglages n'en gardent
//! que l'hôte, pour l'affichage. Une adresse enregistrée en clair par une version
//! précédente y est déplacée au premier chargement.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Color32, CornerRadius, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};
use tidy_integrations::calendar::{Agenda, CalendarError, CalendarProvider, IcsUrl, normalize_url};
use tidy_integrations::ics::{Occurrence, When, day_start};
use tidy_integrations::net::host_of;
use tidy_integrations::secret::{NamedKeyring, SecretStore};
use tidy_integrations::task::{Polled, Task};
use tidy_ui::{icons, t, theme, widgets};

use crate::online::{self, error_notice, net_error_text};
use crate::{Widget, WidgetAction, WidgetCx};

/// Intervalle entre deux chargements d'un agenda.
const REFRESH: Duration = Duration::from_secs(15 * 60);
/// Délai avant de réessayer seul après une erreur.
const RETRY: Duration = Duration::from_secs(5 * 60);
/// Rappels proposés, en minutes (0 : aucun).
const REMINDERS: [u32; 7] = [0, 5, 10, 15, 30, 60, 1440];

/// Un agenda ICS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CalendarSource {
    pub name: String,
    /// Ancienne adresse ICS en clair (versions précédentes) : lue pour être déplacée
    /// dans le gestionnaire d'identifiants, puis retirée. Gardée seulement si ce
    /// déplacement échoue (gestionnaire indisponible), pour ne pas perdre l'agenda.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
    pub color: [u8; 3],
    pub enabled: bool,
    /// Identifiant de l'agenda : son adresse est l'entrée « ics-<id> » du gestionnaire
    /// d'identifiants.
    pub id: String,
    /// Hôte de l'adresse, seul affiché.
    pub host: String,
}

impl Default for CalendarSource {
    fn default() -> Self {
        Self {
            name: String::new(),
            url: String::new(),
            color: rgb(theme::ACCENT_2),
            enabled: true,
            id: String::new(),
            host: String::new(),
        }
    }
}

/// Nouvel identifiant d'agenda.
fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn rgb(c: Color32) -> [u8; 3] {
    [c.r(), c.g(), c.b()]
}

/// Vue du widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    #[default]
    Month,
    Agenda,
}

/// Réglages enregistrés.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CalendarSettings {
    pub calendars: Vec<CalendarSource>,
    pub view: View,
    /// Jours montrés par la vue agenda.
    pub agenda_days: u8,
    /// Rappel, en minutes avant le début (0 : aucun).
    pub reminder_minutes: u32,
}

impl Default for CalendarSettings {
    fn default() -> Self {
        Self {
            calendars: Vec::new(),
            view: View::Month,
            agenda_days: 7,
            reminder_minutes: 10,
        }
    }
}

/// Crée le fournisseur d'une adresse (remplacé dans les tests).
type ProviderFactory =
    Arc<dyn Fn(&str) -> Result<Box<dyn CalendarProvider>, CalendarError> + Send + Sync>;
/// Entrée du gestionnaire d'identifiants d'un agenda, par son identifiant (remplacée
/// dans les tests).
type SecretFactory = Arc<dyn Fn(&str) -> Box<dyn SecretStore> + Send + Sync>;

/// Échec du chargement d'un agenda.
#[derive(Debug, PartialEq)]
enum LoadError {
    Calendar(CalendarError),
    /// Pas d'adresse dans le gestionnaire d'identifiants (entrée effacée à la main).
    NoAddress,
    /// Gestionnaire d'identifiants indisponible.
    Keyring,
}

/// Résultat d'un chargement.
struct Loaded {
    agenda: Result<Box<dyn Agenda>, LoadError>,
    /// L'ancienne adresse en clair est désormais dans le gestionnaire d'identifiants.
    migrated: bool,
}

/// État d'un agenda chargé.
#[derive(Default)]
struct Slot {
    agenda: Option<Box<dyn Agenda>>,
    error: Option<LoadError>,
    tried: Option<Instant>,
    task: Option<Task<Loaded>>,
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

/// Une occurrence et l'agenda d'où elle vient.
#[derive(Debug, Clone, PartialEq)]
struct Item {
    calendar: usize,
    occ: Occurrence,
}

/// Occurrences calculées pour une fenêtre, gardées tant que rien ne change.
struct Cache {
    from: Timestamp,
    to: Timestamp,
    version: u64,
    items: Vec<Item>,
}

/// Formulaire d'ajout d'un agenda.
#[derive(Default)]
struct NewCalendar {
    name: String,
    url: String,
    error: Option<String>,
}

pub struct CalendarWidget {
    s: CalendarSettings,
    factory: ProviderFactory,
    secrets: SecretFactory,
    /// Agendas chargés, par identifiant.
    slots: HashMap<String, Slot>,
    /// Change à chaque agenda chargé ou réglage modifié (invalide les caches).
    version: u64,
    tz: Option<TimeZone>,
    /// Mois affiché (premier jour) et jour choisi.
    month: Option<Date>,
    selected: Option<Date>,
    view_cache: Option<Cache>,
    soon_cache: Option<Cache>,
    /// Rappels déjà envoyés (`uid` + début).
    notified: HashSet<String>,
    new: NewCalendar,
}

/// Création depuis les réglages enregistrés (aucun réseau ici).
pub fn create(settings: &serde_json::Value) -> Box<dyn Widget> {
    let factory: ProviderFactory =
        Arc::new(|url: &str| IcsUrl::new(url).map(|p| Box::new(p) as Box<dyn CalendarProvider>));
    let secrets: SecretFactory =
        Arc::new(|id: &str| Box::new(NamedKeyring::ics_calendar(id)) as Box<dyn SecretStore>);
    Box::new(CalendarWidget::new(
        serde_json::from_value(settings.clone()).unwrap_or_default(),
        factory,
        secrets,
    ))
}

impl CalendarWidget {
    fn new(s: CalendarSettings, factory: ProviderFactory, secrets: SecretFactory) -> Self {
        Self {
            s,
            factory,
            secrets,
            slots: HashMap::new(),
            version: 0,
            tz: None,
            month: None,
            selected: None,
            view_cache: None,
            soon_cache: None,
            notified: HashSet::new(),
            new: NewCalendar::default(),
        }
    }

    fn tz(&mut self) -> TimeZone {
        self.tz.get_or_insert_with(TimeZone::system).clone()
    }

    fn today(&mut self) -> Date {
        Timestamp::now().to_zoned_datetime(self.tz()).date()
    }

    /// Retire l'agenda `i` : son adresse quitte aussi le gestionnaire d'identifiants.
    fn remove_calendar(&mut self, i: usize) {
        if i >= self.s.calendars.len() {
            return;
        }
        let c = self.s.calendars.remove(i);
        if !c.id.is_empty()
            && let Err(e) = (self.secrets)(&c.id).delete()
        {
            log::warn!("agenda {} : adresse non effacée ({e})", c.host);
        }
    }

    /// Identifiant et hôte des agendas qui n'en ont pas encore (versions précédentes).
    fn complete_sources(&mut self, cx: &mut WidgetCx) {
        for c in &mut self.s.calendars {
            if c.id.is_empty() {
                c.id = new_id();
                cx.changed = true;
            }
            if c.host.is_empty() && !c.url.is_empty() {
                c.host = host_of(&c.url).to_owned();
                cx.changed = true;
            }
        }
    }

    /// Relève les chargements finis et lance ceux qui sont dus.
    fn pump(&mut self, ui: &Ui, cx: &mut WidgetCx) {
        self.complete_sources(cx);
        let now = Instant::now();
        let ids: Vec<String> = self
            .s
            .calendars
            .iter()
            .filter(|c| c.enabled)
            .map(|c| c.id.clone())
            .collect();
        let before = self.slots.len();
        self.slots.retain(|k, _| ids.contains(k));
        if self.slots.len() != before {
            self.version += 1;
        }
        for id in ids {
            let slot = self.slots.entry(id.clone()).or_default();
            if let Some(task) = &slot.task {
                match task.poll() {
                    Polled::Pending => continue,
                    Polled::Done(loaded) => {
                        match loaded.agenda {
                            Ok(agenda) => {
                                slot.agenda = Some(agenda);
                                slot.error = None;
                            }
                            Err(e) => slot.error = Some(e),
                        }
                        if loaded.migrated
                            && let Some(c) = self.s.calendars.iter_mut().find(|c| c.id == id)
                        {
                            // Adresse rangée dans le gestionnaire d'identifiants : plus
                            // jamais dans les réglages.
                            c.url.clear();
                            cx.changed = true;
                        }
                    }
                    Polled::Lost => {
                        slot.error = Some(LoadError::Calendar(CalendarError::NotACalendar))
                    }
                }
                slot.task = None;
                slot.tried = Some(now);
                self.version += 1;
            }
            if slot.due(now) == Some(Duration::ZERO) {
                let legacy = legacy_url(&self.s.calendars, &id);
                spawn(slot, &self.factory, &self.secrets, id, legacy, ui);
            }
        }
    }

    /// Occurrences de `[from, to[`, de tous les agendas actifs.
    fn collect(&self, from: Timestamp, to: Timestamp, tz: &TimeZone) -> Vec<Item> {
        let mut items = Vec::new();
        for (i, c) in self.s.calendars.iter().enumerate() {
            if !c.enabled {
                continue;
            }
            if let Some(agenda) = self.slots.get(&c.id).and_then(|s| s.agenda.as_ref()) {
                items.extend(
                    agenda
                        .occurrences(from, to, tz)
                        .into_iter()
                        .map(|occ| Item { calendar: i, occ }),
                );
            }
        }
        items.sort_by(|a, b| {
            a.occ
                .when
                .start(tz)
                .cmp(&b.occ.when.start(tz))
                .then_with(|| b.occ.when.is_all_day().cmp(&a.occ.when.is_all_day()))
        });
        items
    }

    /// Occurrences d'une fenêtre, recalculées seulement si elle ou les données changent.
    fn cached(&mut self, soon: bool, from: Timestamp, to: Timestamp) -> Vec<Item> {
        let tz = self.tz();
        let version = self.version;
        let slot = if soon {
            &self.soon_cache
        } else {
            &self.view_cache
        };
        if let Some(c) = slot
            && c.from == from
            && c.to == to
            && c.version == version
        {
            return c.items.clone();
        }
        let items = self.collect(from, to, &tz);
        let cache = Some(Cache {
            from,
            to,
            version,
            items: items.clone(),
        });
        if soon {
            self.soon_cache = cache;
        } else {
            self.view_cache = cache;
        }
        items
    }

    /// Occurrences des prochaines 25 heures (rappels), à la minute près.
    fn soon(&mut self) -> Vec<Item> {
        let now = Timestamp::now();
        let minute =
            Timestamp::from_second(now.as_second() - now.as_second().rem_euclid(60)).unwrap_or(now);
        let to = minute
            .checked_add(Duration::from_secs(25 * 3600))
            .unwrap_or(minute);
        self.cached(true, minute, to)
    }

    fn color(&self, calendar: usize) -> Color32 {
        self.s.calendars.get(calendar).map_or(theme::ACCENT_2, |c| {
            Color32::from_rgb(c.color[0], c.color[1], c.color[2])
        })
    }

    /// Envoie les rappels dus.
    fn remind(&mut self, cx: &mut WidgetCx) {
        if self.s.reminder_minutes == 0 || self.slots.is_empty() {
            return;
        }
        let soon = self.soon();
        let now = Timestamp::now();
        let tz = self.tz();
        for (key, item) in due_reminders(&soon, now, self.s.reminder_minutes, &self.notified, &tz) {
            let start = item
                .occ
                .when
                .start(&tz)
                .to_zoned_datetime(tz.clone())
                .datetime();
            let minutes = (item.occ.when.start(&tz).as_second() - now.as_second()).max(0) / 60;
            let mut body = t!(
                "widgets2.reminder_body",
                minutes = minutes,
                time = online::hm(start)
            );
            if !item.occ.location.is_empty() {
                body.push_str(" · ");
                body.push_str(&item.occ.location);
            }
            cx.actions.push(WidgetAction::Notify {
                title: if item.occ.summary.is_empty() {
                    t!("widgets2.untitled").to_owned()
                } else {
                    item.occ.summary.clone()
                },
                body,
            });
            self.notified.insert(key);
        }
    }

    fn header_status(&self, ui: &mut Ui) {
        let loading = self.slots.values().any(|s| s.task.is_some());
        if loading {
            ui.add(egui::Spinner::new().size(14.0));
        }
    }

    fn month_ui(&mut self, ui: &mut Ui) {
        let today = self.today();
        let month = *self.month.get_or_insert(today.first_of_month());
        let selected = *self.selected.get_or_insert(today);
        ui.horizontal(|ui| {
            if widgets::icon_button(ui, icons::CHEVRON_LEFT, t!("widgets2.cal_prev")).clicked() {
                self.month = month.checked_sub(1.month()).ok();
            }
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    online::month_name(month.month()),
                    month.year()
                ))
                .size(theme::SIZE_L),
            );
            if widgets::icon_button(ui, icons::CHEVRON_RIGHT, t!("widgets2.cal_next")).clicked() {
                self.month = month.checked_add(1.month()).ok();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if (month != today.first_of_month() || selected != today)
                    && ui.small_button(t!("widgets2.today")).clicked()
                {
                    self.month = Some(today.first_of_month());
                    self.selected = Some(today);
                }
                self.header_status(ui);
            });
        });
        let month = self.month.unwrap_or(month);

        // Grille de 6 semaines, du lundi au dimanche.
        let back = i64::from(month.weekday().to_monday_zero_offset());
        let first = month.checked_sub(back.days()).unwrap_or(month);
        let last = first.checked_add(42.days()).unwrap_or(first);
        let tz = self.tz();
        let items = self.cached(false, day_start(first, &tz), day_start(last, &tz));

        let cell = ((ui.available_width() - 6.0 * 2.0) / 7.0).clamp(24.0, 60.0);
        egui::Grid::new("mois")
            .spacing(vec2(2.0, 2.0))
            .min_col_width(cell)
            .max_col_width(cell)
            .show(ui, |ui| {
                for k in 0..7i8 {
                    let wd = jiff::civil::Weekday::from_monday_zero_offset(k)
                        .unwrap_or(jiff::civil::Weekday::Monday);
                    ui.vertical_centered(|ui| ui.label(widgets::dim(online::weekday_short(wd))));
                }
                ui.end_row();
                for week in 0..6 {
                    for k in 0..7 {
                        let Ok(day) = first.checked_add((week * 7 + k).days()) else {
                            continue;
                        };
                        let colors: Vec<Color32> = {
                            let mut seen = Vec::new();
                            for it in items.iter().filter(|it| it.occ.when.on_day(day, &tz)) {
                                let c = self.color(it.calendar);
                                if !seen.contains(&c) {
                                    seen.push(c);
                                }
                            }
                            seen
                        };
                        let r = day_cell(ui, cell, day, month, today, selected, &colors);
                        if r.clicked() {
                            self.selected = Some(day);
                            if day.first_of_month() != month {
                                self.month = Some(day.first_of_month());
                            }
                        }
                    }
                    ui.end_row();
                }
            });

        // Événements du jour choisi.
        let selected = self.selected.unwrap_or(today);
        widgets::row_separator(ui);
        let day_items: Vec<Item> = if (first..last).contains(&selected) {
            items
                .into_iter()
                .filter(|it| it.occ.when.on_day(selected, &tz))
                .collect()
        } else {
            self.collect(
                day_start(selected, &tz),
                day_start(selected.tomorrow().unwrap_or(selected), &tz),
                &tz,
            )
        };
        ui.label(RichText::new(day_title(selected, today)).size(theme::SIZE_M));
        if day_items.is_empty() {
            ui.label(widgets::dim(t!("widgets2.cal_nothing")));
        }
        egui::ScrollArea::vertical()
            .id_salt("jour")
            .max_height(ui.available_height().max(60.0))
            .show(ui, |ui| {
                for it in &day_items {
                    event_row(ui, it, self.color(it.calendar), &tz);
                }
            });
    }

    fn agenda_ui(&mut self, ui: &mut Ui) {
        let today = self.today();
        ui.horizontal(|ui| {
            ui.label(RichText::new(t!("widgets2.cal_upcoming")).size(theme::SIZE_L));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.header_status(ui);
            });
        });
        let days = i64::from(self.s.agenda_days.clamp(1, 31));
        let end = today.checked_add(days.days()).unwrap_or(today);
        let tz = self.tz();
        // Depuis maintenant : les événements finis de la journée ne s'affichent plus.
        let now = Timestamp::now();
        let minute =
            Timestamp::from_second(now.as_second() - now.as_second().rem_euclid(60)).unwrap_or(now);
        let items = self.cached(false, minute, day_start(end, &tz));
        if items.is_empty() {
            ui.label(widgets::dim(t!("widgets2.cal_nothing_soon")));
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("agenda")
            .max_height(ui.available_height().max(60.0))
            .show(ui, |ui| {
                let mut day = today;
                while day < end {
                    let todays: Vec<&Item> = items
                        .iter()
                        .filter(|it| it.occ.when.on_day(day, &tz))
                        .collect();
                    if !todays.is_empty() {
                        ui.add_space(2.0);
                        ui.label(RichText::new(day_title(day, today)).color(theme::TEXT_DIM));
                        for it in todays {
                            event_row(ui, it, self.color(it.calendar), &tz);
                        }
                    }
                    let Ok(next) = day.tomorrow() else { break };
                    day = next;
                }
            });
    }
}

/// Ancienne adresse en clair de l'agenda `id`, à déplacer dans le gestionnaire
/// d'identifiants.
fn legacy_url(calendars: &[CalendarSource], id: &str) -> Option<String> {
    calendars
        .iter()
        .find(|c| c.id == id && !c.url.is_empty())
        .map(|c| c.url.clone())
}

/// Charge un agenda sur un fil à part : son adresse est lue dans le gestionnaire
/// d'identifiants (ou y est d'abord rangée, si elle était encore en clair).
fn spawn(
    slot: &mut Slot,
    factory: &ProviderFactory,
    secrets: &SecretFactory,
    id: String,
    legacy: Option<String>,
    ui: &Ui,
) {
    let factory = factory.clone();
    let secrets = secrets.clone();
    slot.task = Some(Task::spawn("agenda", online::waker(ui), move || {
        load(&factory, &secrets, &id, legacy)
    }));
}

fn load(
    factory: &ProviderFactory,
    secrets: &SecretFactory,
    id: &str,
    legacy: Option<String>,
) -> Loaded {
    let secret = secrets(id);
    let (url, migrated) = match legacy {
        Some(url) => match secret.set(&url) {
            Ok(()) => {
                log::info!(
                    "agenda {} : adresse rangée dans le gestionnaire d'identifiants",
                    host_of(&url)
                );
                (url, true)
            }
            Err(e) => {
                log::warn!(
                    "agenda {} : adresse gardée dans les réglages ({e})",
                    host_of(&url)
                );
                (url, false)
            }
        },
        None => match secret.get() {
            Ok(Some(url)) => (url, false),
            Ok(None) => {
                return Loaded {
                    agenda: Err(LoadError::NoAddress),
                    migrated: false,
                };
            }
            Err(e) => {
                log::warn!("agenda : {e}");
                return Loaded {
                    agenda: Err(LoadError::Keyring),
                    migrated: false,
                };
            }
        },
    };
    Loaded {
        agenda: factory(&url)
            .and_then(|p| p.load())
            .map_err(LoadError::Calendar),
        migrated,
    }
}

/// Rappels à envoyer maintenant : début dans moins de `minutes`, pas encore commencé,
/// pas déjà envoyé. Les journées entières n'ont pas de rappel.
fn due_reminders<'a>(
    items: &'a [Item],
    now: Timestamp,
    minutes: u32,
    notified: &HashSet<String>,
    tz: &TimeZone,
) -> Vec<(String, &'a Item)> {
    let lead = i64::from(minutes) * 60;
    items
        .iter()
        .filter(|it| !it.occ.when.is_all_day())
        .filter_map(|it| {
            let start = it.occ.when.start(tz).as_second();
            let key = format!("{}@{start}", it.occ.uid);
            let due = now.as_second() >= start - lead && now.as_second() < start;
            (due && !notified.contains(&key)).then_some((key, it))
        })
        .collect()
}

/// Prochain rappel à venir (pour réveiller le widget à temps).
fn next_reminder(items: &[Item], now: Timestamp, minutes: u32, tz: &TimeZone) -> Option<Duration> {
    let lead = i64::from(minutes) * 60;
    items
        .iter()
        .filter(|it| !it.occ.when.is_all_day())
        .map(|it| it.occ.when.start(tz).as_second() - lead - now.as_second())
        .filter(|s| *s > 0)
        .min()
        .and_then(|s| u64::try_from(s).ok())
        .map(Duration::from_secs)
}

/// « Aujourd'hui », « Demain » ou « jeu. 26 novembre ».
fn day_title(day: Date, today: Date) -> String {
    if day == today {
        t!("widgets2.today").to_owned()
    } else if today.tomorrow().ok() == Some(day) {
        t!("widgets2.tomorrow").to_owned()
    } else {
        format!(
            "{} {} {}",
            online::weekday_short(day.weekday()),
            day.day(),
            online::month_name(day.month())
        )
    }
}

/// Case d'un jour : numéro, pastilles des agendas, aujourd'hui en lime, choix en violet.
fn day_cell(
    ui: &mut Ui,
    size: f32,
    day: Date,
    month: Date,
    today: Date,
    selected: Date,
    colors: &[Color32],
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, 30.0), Sense::click());
    let painter = ui.painter();
    if day == today {
        painter.rect_filled(rect, CornerRadius::same(8), theme::ACCENT_2);
    } else if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(8), theme::SURFACE_HI);
    }
    if day == selected {
        painter.rect_stroke(
            rect,
            CornerRadius::same(8),
            Stroke::new(1.5, theme::ACCENT),
            StrokeKind::Inside,
        );
    }
    let text_color = if day == today {
        theme::ON_ACCENT_2
    } else if day.first_of_month() != month {
        theme::TEXT_DIM.gamma_multiply(0.6)
    } else {
        theme::TEXT
    };
    painter.text(
        rect.center() - vec2(0.0, 4.0),
        egui::Align2::CENTER_CENTER,
        day.day().to_string(),
        egui::FontId::proportional(theme::SIZE_S),
        text_color,
    );
    let n = colors.len().min(3);
    for (k, c) in colors.iter().take(3).enumerate() {
        let x = rect.center().x + (k as f32 - (n as f32 - 1.0) / 2.0) * 6.0;
        painter.circle_filled(egui::pos2(x, rect.bottom() - 6.0), 2.2, *c);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Ligne d'un événement : barre de couleur, heure, titre et lieu.
fn event_row(ui: &mut Ui, it: &Item, color: Color32, tz: &TimeZone) {
    ui.horizontal(|ui| {
        let (bar, _) = ui.allocate_exact_size(vec2(3.0, 30.0), Sense::hover());
        ui.painter().rect_filled(bar, 2, color);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let time = match &it.occ.when {
                When::AllDay { .. } => t!("widgets2.all_day").to_owned(),
                When::Timed { start, end } => format!(
                    "{} – {}",
                    online::hm(start.to_zoned_datetime(tz.clone()).datetime()),
                    online::hm(end.to_zoned_datetime(tz.clone()).datetime())
                ),
            };
            let title = if it.occ.summary.is_empty() {
                t!("widgets2.untitled").to_owned()
            } else {
                it.occ.summary.clone()
            };
            ui.add(egui::Label::new(title).truncate());
            let mut line = time;
            if !it.occ.location.is_empty() {
                line.push_str(" · ");
                line.push_str(&it.occ.location);
            }
            ui.add(egui::Label::new(widgets::dim(line)).truncate());
        });
    });
}

impl Widget for CalendarWidget {
    fn ui(&mut self, ui: &mut Ui, cx: &mut WidgetCx) {
        self.pump(ui, cx);
        self.remind(cx);
        if self.s.calendars.iter().all(|c| !c.enabled) {
            ui.vertical_centered(|ui| {
                ui.add_space(12.0);
                ui.label(
                    RichText::new(icons::CALENDAR)
                        .size(32.0)
                        .color(theme::ACCENT),
                );
                ui.add_space(4.0);
                widgets::hint(ui, t!("widgets2.cal_empty"));
            });
            return;
        }
        match self.s.view {
            View::Month => self.month_ui(ui),
            View::Agenda => self.agenda_ui(ui),
        }
        // Erreurs : une ligne par agenda en échec, données précédentes gardées.
        let mut retry = None;
        for c in self.s.calendars.iter().filter(|c| c.enabled) {
            if let Some(e) = self.slots.get(&c.id).and_then(|s| s.error.as_ref()) {
                let text = format!("{} : {}", calendar_name(c), load_error_text(e));
                if error_notice(ui, &text, true) {
                    retry = Some(c.id.clone());
                }
            }
        }
        if let Some(id) = retry {
            let legacy = legacy_url(&self.s.calendars, &id);
            if let Some(slot) = self.slots.get_mut(&id) {
                spawn(slot, &self.factory, &self.secrets, id, legacy, ui);
            }
        }
    }

    fn settings_ui(&mut self, ui: &mut Ui, cx: &mut WidgetCx) -> bool {
        let mut changed = false;
        widgets::section_title(ui, t!("widgets2.cal_calendars"));
        let mut remove = None;
        for (i, c) in self.s.calendars.iter_mut().enumerate() {
            widgets::card(ui, |ui| {
                ui.horizontal(|ui| {
                    if widgets::toggle(ui, &mut c.enabled).changed() {
                        changed = true;
                    }
                    if ui
                        .add(egui::TextEdit::singleline(&mut c.name).desired_width(150.0))
                        .changed()
                    {
                        changed = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::icon_button(ui, icons::DELETE, t!("widgets2.remove")).clicked()
                        {
                            remove = Some(i);
                        }
                    });
                });
                // L'adresse complète est secrète : seul l'hôte s'affiche.
                ui.label(widgets::dim(&c.host));
                let mut color = Color32::from_rgb(c.color[0], c.color[1], c.color[2]);
                if widgets::swatches(ui, &mut color) {
                    c.color = rgb(color);
                    changed = true;
                }
                if let Some(slot) = self.slots.get(&c.id) {
                    if let Some(e) = &slot.error {
                        ui.label(RichText::new(load_error_text(e)).color(theme::WARNING));
                    } else if slot.agenda.is_some() {
                        ui.label(widgets::dim(t!("widgets2.cal_loaded")));
                    }
                }
            });
        }
        if let Some(i) = remove {
            self.remove_calendar(i);
            changed = true;
        }

        // Ajout d'un agenda par son adresse ICS.
        ui.add_space(4.0);
        ui.label(RichText::new(t!("widgets2.cal_add")).size(theme::SIZE_M));
        ui.add(
            egui::TextEdit::singleline(&mut self.new.name)
                .hint_text(t!("widgets2.cal_name_hint"))
                .desired_width(f32::INFINITY),
        );
        ui.add(
            egui::TextEdit::singleline(&mut self.new.url)
                .hint_text(t!("widgets2.cal_url_hint"))
                .password(true)
                .desired_width(f32::INFINITY),
        );
        if let Some(error) = &self.new.error {
            ui.label(RichText::new(error).color(theme::WARNING));
        }
        if widgets::primary(ui, t!("widgets2.cal_add_button")).clicked() {
            match normalize_url(&self.new.url) {
                Ok(url) => {
                    let id = new_id();
                    // L'adresse va dans le gestionnaire d'identifiants, jamais dans les
                    // réglages.
                    match (self.secrets)(&id).set(&url) {
                        Ok(()) => {
                            let n = self.s.calendars.len();
                            let color = widgets::SWATCHES[5 + n % (widgets::SWATCHES.len() - 5)];
                            let name = self.new.name.trim();
                            let host = host_of(&url).to_owned();
                            self.s.calendars.push(CalendarSource {
                                name: if name.is_empty() {
                                    host.clone()
                                } else {
                                    name.to_owned()
                                },
                                color: rgb(color),
                                enabled: true,
                                id,
                                host,
                                ..Default::default()
                            });
                            self.new = NewCalendar::default();
                            changed = true;
                        }
                        Err(e) => {
                            log::warn!("agenda {} : {e}", host_of(&url));
                            self.new.error =
                                Some(t!("widgets2.cal_keyring_unavailable").to_owned());
                        }
                    }
                }
                Err(_) => self.new.error = Some(t!("widgets2.cal_url_bad").to_owned()),
            }
        }
        widgets::hint(ui, t!("widgets2.cal_help_google"));
        widgets::hint(ui, t!("widgets2.cal_help_outlook"));

        widgets::row_separator(ui);
        widgets::setting_row(ui, "", t!("widgets2.cal_view"), None, |ui| {
            for (view, label) in [
                (View::Agenda, t!("widgets2.cal_view_agenda")),
                (View::Month, t!("widgets2.cal_view_month")),
            ] {
                if widgets::choice(ui, self.s.view == view, label).clicked() && self.s.view != view
                {
                    self.s.view = view;
                    changed = true;
                }
            }
        });
        if self.s.view == View::Agenda {
            widgets::setting_row(ui, "", t!("widgets2.cal_days"), None, |ui| {
                if ui
                    .add(egui::Slider::new(&mut self.s.agenda_days, 1..=31))
                    .changed()
                {
                    changed = true;
                }
            });
        }
        widgets::setting_row(ui, icons::BELL, t!("widgets2.cal_reminder"), None, |ui| {
            egui::ComboBox::from_id_salt("rappel")
                .selected_text(reminder_text(self.s.reminder_minutes))
                .show_ui(ui, |ui| {
                    for m in REMINDERS {
                        if ui
                            .selectable_value(&mut self.s.reminder_minutes, m, reminder_text(m))
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
        });
        if changed {
            self.version += 1;
            cx.changed = true;
        }
        changed
    }

    fn save(&self) -> serde_json::Value {
        serde_json::to_value(&self.s).unwrap_or_default()
    }

    fn repaint_after(&self) -> Option<Duration> {
        let now = Instant::now();
        let mut next = self.slots.values().filter_map(|s| s.due(now)).min();
        if self.s.reminder_minutes > 0
            && let (Some(cache), Some(tz)) = (&self.soon_cache, &self.tz)
        {
            let r = next_reminder(&cache.items, Timestamp::now(), self.s.reminder_minutes, tz);
            next = match (next, r) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
        // Au moins une fois par minute tant qu'un agenda est actif : le jour change,
        // la fenêtre des rappels avance.
        if !self.slots.is_empty() {
            next = Some(next.map_or(Duration::from_secs(60), |n| n.min(Duration::from_secs(60))));
        }
        next.map(|d| d.max(Duration::from_secs(1)))
    }
}

fn calendar_name(c: &CalendarSource) -> &str {
    if c.name.trim().is_empty() {
        &c.host
    } else {
        &c.name
    }
}

fn load_error_text(e: &LoadError) -> String {
    match e {
        LoadError::Calendar(e) => calendar_error_text(e),
        LoadError::NoAddress => t!("widgets2.cal_address_missing").to_owned(),
        LoadError::Keyring => t!("widgets2.cal_keyring_unavailable").to_owned(),
    }
}

fn calendar_error_text(e: &CalendarError) -> String {
    match e {
        CalendarError::Net(e) => net_error_text(e),
        CalendarError::BadUrl => t!("widgets2.cal_url_bad").to_owned(),
        CalendarError::NotACalendar => t!("widgets2.cal_not_ics").to_owned(),
    }
}

fn reminder_text(minutes: u32) -> String {
    match minutes {
        0 => t!("widgets2.reminder_none").to_owned(),
        1440 => t!("widgets2.reminder_day").to_owned(),
        60 => t!("widgets2.reminder_hour").to_owned(),
        m => t!("widgets2.reminder_minutes", minutes = m),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing::render;
    use tidy_integrations::ics;

    /// Agenda enregistré, chargé sans réseau.
    struct Recorded(&'static str);

    impl CalendarProvider for Recorded {
        fn load(&self) -> Result<Box<dyn Agenda>, CalendarError> {
            ics::parse(self.0)
                .map(|c| Box::new(c) as Box<dyn Agenda>)
                .map_err(|_| CalendarError::NotACalendar)
        }
    }

    fn recorded() -> ProviderFactory {
        Arc::new(|url: &str| {
            Ok(Box::new(Recorded(if url.contains("outlook") {
                include_str!("../../tidy-integrations/tests/data/outlook.ics")
            } else if url.contains("broken") {
                "<html>connexion</html>"
            } else {
                include_str!("../../tidy-integrations/tests/data/google.ics")
            })) as Box<dyn CalendarProvider>)
        })
    }

    /// Gestionnaire d'identifiants en mémoire, partagé par toutes les entrées.
    #[derive(Clone, Default)]
    struct Vault(Arc<std::sync::Mutex<HashMap<String, String>>>);

    struct VaultEntry(Vault, String);

    impl SecretStore for VaultEntry {
        fn get(&self) -> Result<Option<String>, tidy_integrations::secret::SecretError> {
            Ok(self.0.0.lock().unwrap().get(&self.1).cloned())
        }
        fn set(&self, value: &str) -> Result<(), tidy_integrations::secret::SecretError> {
            self.0
                .0
                .lock()
                .unwrap()
                .insert(self.1.clone(), value.into());
            Ok(())
        }
        fn delete(&self) -> Result<(), tidy_integrations::secret::SecretError> {
            self.0.0.lock().unwrap().remove(&self.1);
            Ok(())
        }
    }

    impl Vault {
        fn factory(&self) -> SecretFactory {
            let vault = self.clone();
            Arc::new(move |id: &str| {
                Box::new(VaultEntry(vault.clone(), format!("ics-{id}"))) as Box<dyn SecretStore>
            })
        }
    }

    fn widget(s: CalendarSettings) -> (CalendarWidget, Vault) {
        let vault = Vault::default();
        (CalendarWidget::new(s, recorded(), vault.factory()), vault)
    }

    /// Réglages d'une version précédente : adresses en clair, sans identifiant.
    fn settings(urls: &[&str], view: View) -> CalendarSettings {
        CalendarSettings {
            calendars: urls
                .iter()
                .map(|u| CalendarSource {
                    name: (*u).into(),
                    url: (*u).into(),
                    ..Default::default()
                })
                .collect(),
            view,
            ..Default::default()
        }
    }

    fn wait(w: &mut CalendarWidget) {
        for _ in 0..200 {
            if !w.slots.is_empty() && w.slots.values().all(|s| s.task.is_none()) {
                break;
            }
            render(w, 1);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn calendars_load_and_draw_in_both_views() {
        for view in [View::Month, View::Agenda] {
            let s = settings(
                &["https://google.test/a.ics", "https://outlook.test/b.ics"],
                view,
            );
            let (mut w, _) = widget(s);
            render(&mut w, 1);
            wait(&mut w);
            render(&mut w, 2);
            assert!(
                w.slots
                    .values()
                    .all(|s| s.agenda.is_some() && s.error.is_none())
            );
        }
    }

    #[test]
    fn a_page_that_is_not_a_calendar_is_reported() {
        let (mut w, _) = widget(settings(&["https://broken.test/x"], View::Month));
        render(&mut w, 1);
        wait(&mut w);
        render(&mut w, 1);
        let slot = w.slots.values().next().unwrap();
        assert_eq!(
            slot.error,
            Some(LoadError::Calendar(CalendarError::NotACalendar))
        );
    }

    #[test]
    fn no_calendar_means_no_network_and_no_wake_up() {
        let (mut w, _) = widget(CalendarSettings::default());
        render(&mut w, 2);
        assert!(w.slots.is_empty());
        assert_eq!(w.repaint_after(), None);
    }

    fn timed(uid: &str, start: &str, minutes: i64) -> Item {
        let start: Timestamp = start.parse().unwrap();
        Item {
            calendar: 0,
            occ: Occurrence {
                uid: uid.into(),
                summary: uid.into(),
                location: String::new(),
                when: When::Timed {
                    start,
                    end: start
                        .checked_add(Duration::from_secs(60 * minutes as u64))
                        .unwrap(),
                },
            },
        }
    }

    #[test]
    fn reminders_fire_once_within_the_lead_time() {
        let tz = TimeZone::UTC;
        let items = vec![
            timed("bientot", "2026-11-26T10:08:00Z", 30),
            timed("plus-tard", "2026-11-26T11:00:00Z", 30),
            timed("commence", "2026-11-26T09:59:00Z", 30),
        ];
        let now: Timestamp = "2026-11-26T10:00:00Z".parse().unwrap();
        let mut notified = HashSet::new();
        let due = due_reminders(&items, now, 10, &notified, &tz);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].1.occ.uid, "bientot");
        notified.insert(due[0].0.clone());
        assert!(due_reminders(&items, now, 10, &notified, &tz).is_empty());
        // Le suivant tombe à 10 h 50 (11 h moins 10 minutes).
        assert_eq!(
            next_reminder(&items, now, 10, &tz),
            Some(Duration::from_secs(50 * 60))
        );
    }

    #[test]
    fn day_titles() {
        let today = jiff::civil::date(2026, 11, 26);
        assert_eq!(day_title(today, today), t!("widgets2.today"));
        assert_eq!(
            day_title(jiff::civil::date(2026, 11, 27), today),
            t!("widgets2.tomorrow")
        );
    }

    #[test]
    fn settings_survive_a_round_trip() {
        let mut s = settings(&[], View::Agenda);
        s.calendars.push(CalendarSource {
            name: "Travail".into(),
            id: "abc".into(),
            host: "google.test".into(),
            ..Default::default()
        });
        let (w, _) = widget(s.clone());
        let again: CalendarSettings = serde_json::from_value(w.save()).unwrap();
        assert_eq!(again, s);
    }

    /// Adresses en clair d'une version précédente : rangées dans le gestionnaire
    /// d'identifiants au premier chargement, puis absentes des réglages.
    #[test]
    fn plain_addresses_move_to_the_keyring() {
        let url = "https://google.test/private-abc/basic.ics";
        let mut s = settings(&[url], View::Agenda);
        s.calendars[0].name = "Travail".into();
        let (mut w, vault) = widget(s);
        let mut changed = false;
        for _ in 0..200 {
            changed |= render(&mut w, 1).changed;
            if w.s.calendars[0].url.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(changed, "réglages à enregistrer");
        let c = &w.s.calendars[0];
        assert!(c.url.is_empty());
        assert_eq!(c.host, "google.test");
        assert_eq!(
            vault
                .0
                .lock()
                .unwrap()
                .get(&format!("ics-{}", c.id))
                .map(String::as_str),
            Some(url)
        );
        let saved = w.save().to_string();
        assert!(
            !saved.contains("private-abc") && !saved.contains("\"url\""),
            "{saved}"
        );
        // Rechargé depuis le gestionnaire d'identifiants.
        let saved: CalendarSettings = serde_json::from_value(w.save()).unwrap();
        let mut again = CalendarWidget::new(saved, recorded(), vault.factory());
        render(&mut again, 1);
        wait(&mut again);
        assert!(again.slots.values().all(|s| s.agenda.is_some()));

        // Agenda retiré : son entrée aussi.
        again.remove_calendar(0);
        assert!(again.s.calendars.is_empty());
        assert!(vault.0.lock().unwrap().is_empty());
    }

    #[test]
    fn a_missing_address_is_reported() {
        let mut s = settings(&[], View::Agenda);
        s.calendars.push(CalendarSource {
            id: "perdu".into(),
            host: "google.test".into(),
            ..Default::default()
        });
        let (mut w, _) = widget(s);
        render(&mut w, 1);
        wait(&mut w);
        assert_eq!(
            w.slots.values().next().unwrap().error,
            Some(LoadError::NoAddress)
        );
    }
}
