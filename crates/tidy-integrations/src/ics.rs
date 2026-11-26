//! Lecture des agendas iCalendar (RFC 5545), juste ce qu'il faut pour un widget :
//! événements (`VEVENT`), journées entières, fuseaux (`TZID` IANA ou Windows, heures
//! UTC ou flottantes), durées, exceptions (`EXDATE`, `RDATE`), occurrences modifiées
//! (`RECURRENCE-ID`), événements annulés, et récurrences `RRULE` courantes :
//! quotidienne, hebdomadaire, mensuelle, annuelle, avec `INTERVAL`, `COUNT`, `UNTIL`,
//! `BYDAY` (avec rang : `2MO`, `-1FR`), `BYMONTHDAY`, `BYMONTH`, `BYSETPOS`, `WKST`.
//!
//! Non pris en charge (événement alors montré une seule fois, ou règle approchée) :
//! fréquences à l'heure ou à la minute, `BYYEARDAY`, `BYWEEKNO`, `BYHOUR`. Les fuseaux
//! définis seulement par un `VTIMEZONE` maison retombent sur le fuseau de l'agenda,
//! puis sur celui du système.

use std::collections::{HashMap, HashSet};

use jiff::civil::{Date, DateTime, Time, Weekday};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Span, Timestamp, ToSpan, ZonedDateTime as Zoned};

/// Plafond d'occurrences rendues par événement (règle sans fin mal formée).
const MAX_OCCURRENCES: usize = 2000;
/// Plafond de périodes parcourues par règle (une règle quotidienne de 1990 à 2100).
const MAX_PERIODS: i64 = 50_000;

/// Moment d'un événement, tel qu'écrit dans le fichier.
#[derive(Debug, Clone)]
pub enum IcsTime {
    /// Date seule (`VALUE=DATE`) : journée entière.
    Date(Date),
    /// Heure sans fuseau : heure locale de qui lit l'agenda.
    Floating(DateTime),
    /// Heure UTC (suffixe `Z`).
    Utc(Timestamp),
    /// Heure dans un fuseau (`TZID=…`).
    Local(DateTime, TimeZone),
}

impl IcsTime {
    /// Heure civile et fuseau qui servent aux récurrences (`None` pour une date seule).
    fn civil(&self, default: &TimeZone) -> Option<(DateTime, TimeZone)> {
        match self {
            IcsTime::Date(_) => None,
            IcsTime::Floating(dt) => Some((*dt, default.clone())),
            IcsTime::Utc(ts) => Some((
                ts.to_zoned_datetime(TimeZone::UTC).datetime(),
                TimeZone::UTC,
            )),
            IcsTime::Local(dt, tz) => Some((*dt, tz.clone())),
        }
    }

    /// Instant (une date seule : minuit dans `default`).
    fn timestamp(&self, default: &TimeZone) -> Option<Timestamp> {
        match self {
            IcsTime::Date(d) => d
                .to_zoned_datetime(default.clone())
                .ok()
                .map(|z| z.timestamp()),
            IcsTime::Utc(ts) => Some(*ts),
            IcsTime::Floating(dt) => to_zoned(*dt, default).map(|z| z.timestamp()),
            IcsTime::Local(dt, tz) => to_zoned(*dt, tz).map(|z| z.timestamp()),
        }
    }

    /// Date civile (dans le fuseau de l'heure).
    fn date(&self, default: &TimeZone) -> Option<Date> {
        match self {
            IcsTime::Date(d) => Some(*d),
            other => other.civil(default).map(|(dt, _)| dt.date()),
        }
    }
}

/// Heure civile placée dans un fuseau (heure absente au changement d'heure : décalée,
/// heure en double : la première).
fn to_zoned(dt: DateTime, tz: &TimeZone) -> Option<Zoned> {
    tz.to_ambiguous_zoned_datetime(dt).compatible().ok()
}

/// Fréquence d'une règle de récurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// Règle de récurrence (`RRULE`).
#[derive(Debug, Clone)]
pub struct RRule {
    pub freq: Freq,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<IcsTime>,
    /// Jours de la semaine, avec leur rang éventuel (`0` : tous ; `2` : le deuxième ;
    /// `-1` : le dernier).
    pub by_day: Vec<(i16, Weekday)>,
    pub by_month_day: Vec<i8>,
    pub by_month: Vec<i8>,
    pub by_set_pos: Vec<i32>,
    pub week_start: Weekday,
}

/// Durée d'un événement.
#[derive(Debug, Clone)]
enum Length {
    /// Écart absolu entre `DTSTART` et `DTEND`.
    Exact(SignedDuration),
    /// `DURATION` (jours calendaires : 1 jour reste 1 jour au changement d'heure).
    Nominal(Span),
    /// Journées entières.
    Days(i32),
}

/// Événement lu dans le fichier (une occurrence ou une règle).
#[derive(Debug, Clone)]
pub struct Event {
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub description: String,
    pub start: IcsTime,
    end: Option<IcsTime>,
    duration: Option<Span>,
    pub rrule: Option<RRule>,
    exdates: Vec<IcsTime>,
    rdates: Vec<IcsTime>,
    /// Occurrence modifiée d'un événement récurrent (même `UID`).
    pub recurrence_id: Option<IcsTime>,
    pub cancelled: bool,
}

impl Event {
    pub fn all_day(&self) -> bool {
        matches!(self.start, IcsTime::Date(_))
    }

    fn length(&self, default: &TimeZone) -> Length {
        if self.all_day() {
            let days = match (&self.end, &self.duration) {
                (Some(end), _) => match (self.start.date(default), end.date(default)) {
                    (Some(s), Some(e)) => s.until(e).map_or(1, |span| span.get_days()),
                    _ => 1,
                },
                (None, Some(span)) => span.get_days() + span.get_weeks() * 7,
                (None, None) => 1,
            };
            return Length::Days(days.max(1));
        }
        if let Some(end) = &self.end
            && let (Some(s), Some(e)) = (self.start.timestamp(default), end.timestamp(default))
        {
            return Length::Exact(s.duration_until(e).max(SignedDuration::ZERO));
        }
        match &self.duration {
            Some(span) => Length::Nominal(*span),
            None => Length::Exact(SignedDuration::ZERO),
        }
    }
}

/// Quand a lieu une occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum When {
    /// Journées entières, `end` exclu (un seul jour : `end = start + 1`).
    AllDay {
        start: Date,
        end: Date,
    },
    Timed {
        start: Timestamp,
        end: Timestamp,
    },
}

impl When {
    /// Début, dans le fuseau `tz` pour une journée entière.
    pub fn start(&self, tz: &TimeZone) -> Timestamp {
        match self {
            When::AllDay { start, .. } => day_start(*start, tz),
            When::Timed { start, .. } => *start,
        }
    }

    pub fn end(&self, tz: &TimeZone) -> Timestamp {
        match self {
            When::AllDay { end, .. } => day_start(*end, tz),
            When::Timed { end, .. } => *end,
        }
    }

    pub fn is_all_day(&self) -> bool {
        matches!(self, When::AllDay { .. })
    }

    /// Recoupe `[from, to[` (un événement sans durée compte à son début).
    pub fn overlaps(&self, from: Timestamp, to: Timestamp, tz: &TimeZone) -> bool {
        let (s, e) = (self.start(tz), self.end(tz));
        if s == e {
            s >= from && s < to
        } else {
            s < to && e > from
        }
    }

    /// A lieu (au moins en partie) le jour `day` du fuseau `tz`.
    pub fn on_day(&self, day: Date, tz: &TimeZone) -> bool {
        match self {
            When::AllDay { start, end } => *start <= day && day < *end,
            When::Timed { .. } => {
                let next = day.tomorrow().unwrap_or(day);
                self.overlaps(day_start(day, tz), day_start(next, tz), tz)
            }
        }
    }
}

/// Minuit du jour `d` dans `tz`.
pub fn day_start(d: Date, tz: &TimeZone) -> Timestamp {
    d.to_zoned_datetime(tz.clone())
        .map_or(Timestamp::UNIX_EPOCH, |z| z.timestamp())
}

/// Une occurrence à afficher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub when: When,
}

/// Agenda lu.
#[derive(Debug, Clone)]
pub struct Calendar {
    /// `X-WR-CALNAME` (nom donné par Google ou Outlook).
    pub name: Option<String>,
    pub events: Vec<Event>,
    /// `X-WR-TIMEZONE` : fuseau des heures flottantes.
    pub time_zone: Option<TimeZone>,
}

/// Fichier qui n'est pas un agenda.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotACalendar;

impl std::fmt::Display for NotACalendar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pas un agenda iCalendar")
    }
}

impl std::error::Error for NotACalendar {}

/// Propriété d'une ligne : nom (en majuscules), paramètres, valeur brute.
#[derive(Debug, PartialEq)]
struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Prop {
    fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Lignes dépliées (une ligne qui commence par une espace prolonge la précédente).
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = raw.strip_prefix([' ', '\t'])
            && let Some(last) = lines.last_mut()
        {
            last.push_str(rest);
            continue;
        }
        if !raw.is_empty() {
            lines.push(raw.to_owned());
        }
    }
    lines
}

/// `NOM;PARAM=val;PARAM="v:a;l":valeur`.
fn parse_line(line: &str) -> Option<Prop> {
    let mut chars = line.char_indices().peekable();
    let mut name_end = None;
    for (i, c) in chars.by_ref() {
        if c == ';' || c == ':' {
            name_end = Some((i, c));
            break;
        }
    }
    let (end, mut sep) = name_end?;
    let name = line[..end].trim().to_ascii_uppercase();
    let mut params = Vec::new();
    while sep == ';' {
        // Nom du paramètre.
        let start = chars.peek()?.0;
        let mut key_end = None;
        for (i, c) in chars.by_ref() {
            if c == '=' {
                key_end = Some(i);
                break;
            }
            if c == ':' || c == ';' {
                return None;
            }
        }
        let key = line[start..key_end?].to_ascii_uppercase();
        // Valeur(s), entre guillemets ou non.
        let mut value = String::new();
        let mut quoted = false;
        loop {
            let (_, c) = chars.next()?;
            match c {
                '"' => quoted = !quoted,
                ';' | ':' if !quoted => {
                    sep = c;
                    break;
                }
                c => value.push(c),
            }
        }
        params.push((key, value));
    }
    let value = match chars.peek() {
        Some(&(i, _)) => line[i..].to_owned(),
        None => String::new(),
    };
    Some(Prop {
        name,
        params,
        value,
    })
}

/// Texte iCalendar : `\n`, `\,`, `\;`, `\\`.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn digits(s: &str) -> Option<i32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// `20261126`.
fn parse_date(s: &str) -> Option<Date> {
    if s.len() != 8 {
        return None;
    }
    let y = i16::try_from(digits(&s[0..4])?).ok()?;
    let m = i8::try_from(digits(&s[4..6])?).ok()?;
    let d = i8::try_from(digits(&s[6..8])?).ok()?;
    Date::new(y, m, d).ok()
}

/// `20261126T090000` (et un `Z` final : UTC).
fn parse_datetime(s: &str) -> Option<(DateTime, bool)> {
    let (s, utc) = match s.strip_suffix(['Z', 'z']) {
        Some(rest) => (rest, true),
        None => (s, false),
    };
    let (date, time) = s.split_once(['T', 't'])?;
    let date = parse_date(date)?;
    if time.len() < 4 {
        return None;
    }
    let h = i8::try_from(digits(&time[0..2])?).ok()?;
    let mi = i8::try_from(digits(&time[2..4])?).ok()?;
    let sec = if time.len() >= 6 {
        i8::try_from(digits(&time[4..6])?).ok()?.min(59)
    } else {
        0
    };
    let time = Time::new(h, mi, sec, 0).ok()?;
    Some((date.to_datetime(time), utc))
}

fn parse_weekday(s: &str) -> Option<Weekday> {
    Some(match s.to_ascii_uppercase().as_str() {
        "MO" => Weekday::Monday,
        "TU" => Weekday::Tuesday,
        "WE" => Weekday::Wednesday,
        "TH" => Weekday::Thursday,
        "FR" => Weekday::Friday,
        "SA" => Weekday::Saturday,
        "SU" => Weekday::Sunday,
        _ => return None,
    })
}

/// Fuseaux de Windows (Outlook, Exchange) vers les noms IANA.
const WINDOWS_ZONES: &[(&str, &str)] = &[
    ("Romance Standard Time", "Europe/Paris"),
    ("W. Europe Standard Time", "Europe/Berlin"),
    ("Central Europe Standard Time", "Europe/Budapest"),
    ("Central European Standard Time", "Europe/Warsaw"),
    ("GMT Standard Time", "Europe/London"),
    ("Greenwich Standard Time", "Atlantic/Reykjavik"),
    ("E. Europe Standard Time", "Europe/Chisinau"),
    ("FLE Standard Time", "Europe/Kyiv"),
    ("GTB Standard Time", "Europe/Bucharest"),
    ("Russian Standard Time", "Europe/Moscow"),
    ("Turkey Standard Time", "Europe/Istanbul"),
    ("Israel Standard Time", "Asia/Jerusalem"),
    ("Egypt Standard Time", "Africa/Cairo"),
    ("South Africa Standard Time", "Africa/Johannesburg"),
    ("Morocco Standard Time", "Africa/Casablanca"),
    ("W. Central Africa Standard Time", "Africa/Lagos"),
    ("Arabian Standard Time", "Asia/Dubai"),
    ("Arab Standard Time", "Asia/Riyadh"),
    ("Iran Standard Time", "Asia/Tehran"),
    ("Pakistan Standard Time", "Asia/Karachi"),
    ("India Standard Time", "Asia/Kolkata"),
    ("Bangladesh Standard Time", "Asia/Dhaka"),
    ("SE Asia Standard Time", "Asia/Bangkok"),
    ("Singapore Standard Time", "Asia/Singapore"),
    ("China Standard Time", "Asia/Shanghai"),
    ("Taipei Standard Time", "Asia/Taipei"),
    ("Tokyo Standard Time", "Asia/Tokyo"),
    ("Korea Standard Time", "Asia/Seoul"),
    ("W. Australia Standard Time", "Australia/Perth"),
    ("Cen. Australia Standard Time", "Australia/Adelaide"),
    ("E. Australia Standard Time", "Australia/Brisbane"),
    ("AUS Eastern Standard Time", "Australia/Sydney"),
    ("New Zealand Standard Time", "Pacific/Auckland"),
    ("Mauritius Standard Time", "Indian/Mauritius"),
    ("Azores Standard Time", "Atlantic/Azores"),
    ("Cape Verde Standard Time", "Atlantic/Cape_Verde"),
    ("Eastern Standard Time", "America/New_York"),
    ("Central Standard Time", "America/Chicago"),
    ("Mountain Standard Time", "America/Denver"),
    ("US Mountain Standard Time", "America/Phoenix"),
    ("Pacific Standard Time", "America/Los_Angeles"),
    ("Alaskan Standard Time", "America/Anchorage"),
    ("Hawaiian Standard Time", "Pacific/Honolulu"),
    ("Atlantic Standard Time", "America/Halifax"),
    ("Newfoundland Standard Time", "America/St_Johns"),
    ("Canada Central Standard Time", "America/Regina"),
    ("Central Standard Time (Mexico)", "America/Mexico_City"),
    ("SA Pacific Standard Time", "America/Bogota"),
    ("Pacific SA Standard Time", "America/Santiago"),
    ("Argentina Standard Time", "America/Buenos_Aires"),
    ("E. South America Standard Time", "America/Sao_Paulo"),
    ("UTC", "UTC"),
    ("Coordinated Universal Time", "UTC"),
];

/// Fuseau d'après un `TZID` : nom IANA, nom IANA précédé d'un préfixe
/// (`/mozilla.org/20050126_1/Europe/Paris`), ou nom Windows.
pub fn resolve_time_zone(tzid: &str) -> Option<TimeZone> {
    let tzid = tzid.trim().trim_matches('"');
    if tzid.is_empty() {
        return None;
    }
    if let Ok(tz) = TimeZone::get(tzid) {
        return Some(tz);
    }
    for (i, c) in tzid.char_indices() {
        if c == '/'
            && let Ok(tz) = TimeZone::get(&tzid[i + 1..])
        {
            return Some(tz);
        }
    }
    WINDOWS_ZONES
        .iter()
        .find(|(win, _)| win.eq_ignore_ascii_case(tzid))
        .and_then(|(_, iana)| TimeZone::get(iana).ok())
}

/// Lecteur des valeurs de temps, avec un cache des fuseaux.
struct TimeReader {
    zones: HashMap<String, Option<TimeZone>>,
}

impl TimeReader {
    fn zone(&mut self, tzid: &str) -> Option<TimeZone> {
        self.zones
            .entry(tzid.to_owned())
            .or_insert_with(|| {
                let tz = resolve_time_zone(tzid);
                if tz.is_none() {
                    log::debug!("fuseau d'agenda inconnu : {tzid}");
                }
                tz
            })
            .clone()
    }

    /// Une valeur de temps (`DTSTART`, `UNTIL`…), avec le `TZID` éventuel.
    fn time(&mut self, value: &str, tzid: Option<&str>, date_only: bool) -> Option<IcsTime> {
        let value = value.trim();
        if date_only || (value.len() == 8 && !value.contains(['T', 't'])) {
            return parse_date(value).map(IcsTime::Date);
        }
        let (dt, utc) = parse_datetime(value)?;
        if utc {
            return dt
                .to_zoned_datetime(TimeZone::UTC)
                .ok()
                .map(|z| IcsTime::Utc(z.timestamp()));
        }
        Some(match tzid.and_then(|id| self.zone(id)) {
            Some(tz) => IcsTime::Local(dt, tz),
            None => IcsTime::Floating(dt),
        })
    }

    fn prop_time(&mut self, prop: &Prop) -> Option<IcsTime> {
        let date_only = prop
            .param("VALUE")
            .is_some_and(|v| v.eq_ignore_ascii_case("DATE"));
        self.time(&prop.value, prop.param("TZID"), date_only)
    }

    /// Valeurs multiples (`EXDATE:a,b,c`).
    fn prop_times(&mut self, prop: &Prop) -> Vec<IcsTime> {
        let date_only = prop
            .param("VALUE")
            .is_some_and(|v| v.eq_ignore_ascii_case("DATE"));
        let tzid = prop.param("TZID").map(str::to_owned);
        prop.value
            .split(',')
            .filter_map(|v| self.time(v, tzid.as_deref(), date_only))
            .collect()
    }

    fn rrule(&mut self, value: &str) -> Option<RRule> {
        let mut freq = None;
        let mut rule = RRule {
            freq: Freq::Daily,
            interval: 1,
            count: None,
            until: None,
            by_day: Vec::new(),
            by_month_day: Vec::new(),
            by_month: Vec::new(),
            by_set_pos: Vec::new(),
            week_start: Weekday::Monday,
        };
        for part in value.split(';') {
            let Some((key, val)) = part.split_once('=') else {
                continue;
            };
            let list = || val.split(',').map(str::trim).filter(|s| !s.is_empty());
            match key.trim().to_ascii_uppercase().as_str() {
                "FREQ" => {
                    freq = match val.trim().to_ascii_uppercase().as_str() {
                        "DAILY" => Some(Freq::Daily),
                        "WEEKLY" => Some(Freq::Weekly),
                        "MONTHLY" => Some(Freq::Monthly),
                        "YEARLY" => Some(Freq::Yearly),
                        other => {
                            log::debug!("récurrence non prise en charge : FREQ={other}");
                            None
                        }
                    }
                }
                "INTERVAL" => rule.interval = val.trim().parse().unwrap_or(1).max(1),
                "COUNT" => rule.count = val.trim().parse().ok(),
                "UNTIL" => rule.until = self.time(val, None, false),
                "BYDAY" => {
                    rule.by_day = list()
                        .filter_map(|d| {
                            let split = d.len().checked_sub(2)?;
                            let wd = parse_weekday(d.get(split..)?)?;
                            let n = d[..split].trim_start_matches('+');
                            let n = if n.is_empty() { 0 } else { n.parse().ok()? };
                            Some((n, wd))
                        })
                        .collect();
                }
                "BYMONTHDAY" => rule.by_month_day = list().filter_map(|d| d.parse().ok()).collect(),
                "BYMONTH" => rule.by_month = list().filter_map(|d| d.parse().ok()).collect(),
                "BYSETPOS" => rule.by_set_pos = list().filter_map(|d| d.parse().ok()).collect(),
                "WKST" => rule.week_start = parse_weekday(val.trim()).unwrap_or(Weekday::Monday),
                "BYYEARDAY" | "BYWEEKNO" | "BYHOUR" | "BYMINUTE" | "BYSECOND" => {
                    log::debug!("récurrence approchée : {key} ignoré");
                }
                _ => {}
            }
        }
        rule.freq = freq?;
        Some(rule)
    }
}

/// Lit un agenda iCalendar. Les événements illisibles sont ignorés.
pub fn parse(text: &str) -> Result<Calendar, NotACalendar> {
    let lines = unfold(text.trim_start_matches('\u{feff}'));
    if !lines
        .iter()
        .any(|l| l.trim().eq_ignore_ascii_case("BEGIN:VCALENDAR"))
    {
        return Err(NotACalendar);
    }
    let mut reader = TimeReader {
        zones: HashMap::new(),
    };
    let mut calendar = Calendar {
        name: None,
        events: Vec::new(),
        time_zone: None,
    };
    // Composants ouverts : on ne lit que les propriétés directes d'un VEVENT (pas
    // celles d'un VALARM qu'il contient) et celles du VCALENDAR.
    let mut stack: Vec<String> = Vec::new();
    let mut props: Vec<Prop> = Vec::new();
    for line in &lines {
        let Some(prop) = parse_line(line) else {
            continue;
        };
        match prop.name.as_str() {
            "BEGIN" => {
                let comp = prop.value.trim().to_ascii_uppercase();
                if comp == "VEVENT" && stack.last().is_some_and(|c| c == "VCALENDAR") {
                    props.clear();
                }
                stack.push(comp);
            }
            "END" => {
                let comp = prop.value.trim().to_ascii_uppercase();
                if comp == "VEVENT" && stack.last().is_some_and(|c| c == "VEVENT") {
                    if let Some(event) = build_event(&mut reader, &props) {
                        calendar.events.push(event);
                    }
                    props.clear();
                }
                // Ferme jusqu'au composant nommé (fichier mal imbriqué toléré).
                if let Some(pos) = stack.iter().rposition(|c| *c == comp) {
                    stack.truncate(pos);
                }
            }
            _ => match stack.last().map(String::as_str) {
                Some("VEVENT") => props.push(prop),
                Some("VCALENDAR") => match prop.name.as_str() {
                    "X-WR-CALNAME" => calendar.name = Some(unescape(&prop.value)),
                    "X-WR-TIMEZONE" => calendar.time_zone = reader.zone(prop.value.trim()),
                    _ => {}
                },
                _ => {}
            },
        }
    }
    Ok(calendar)
}

fn build_event(reader: &mut TimeReader, props: &[Prop]) -> Option<Event> {
    let mut event = Event {
        uid: String::new(),
        summary: String::new(),
        location: String::new(),
        description: String::new(),
        start: IcsTime::Date(Date::constant(1970, 1, 1)),
        end: None,
        duration: None,
        rrule: None,
        exdates: Vec::new(),
        rdates: Vec::new(),
        recurrence_id: None,
        cancelled: false,
    };
    let mut has_start = false;
    for prop in props {
        match prop.name.as_str() {
            "UID" => event.uid = prop.value.trim().to_owned(),
            "SUMMARY" => event.summary = unescape(&prop.value),
            "LOCATION" => event.location = unescape(&prop.value),
            "DESCRIPTION" => event.description = unescape(&prop.value),
            "DTSTART" => {
                if let Some(t) = reader.prop_time(prop) {
                    event.start = t;
                    has_start = true;
                }
            }
            "DTEND" => event.end = reader.prop_time(prop),
            "DURATION" => event.duration = prop.value.trim().parse::<Span>().ok(),
            "RRULE" => event.rrule = reader.rrule(&prop.value),
            "EXDATE" => event.exdates.extend(reader.prop_times(prop)),
            "RDATE" => {
                let is_period = prop
                    .param("VALUE")
                    .is_some_and(|v| v.eq_ignore_ascii_case("PERIOD"));
                if !is_period {
                    event.rdates.extend(reader.prop_times(prop));
                }
            }
            "RECURRENCE-ID" => event.recurrence_id = reader.prop_time(prop),
            "STATUS" => event.cancelled = prop.value.trim().eq_ignore_ascii_case("CANCELLED"),
            _ => {}
        }
    }
    if !has_start {
        log::debug!("événement sans DTSTART ignoré");
        return None;
    }
    Some(event)
}

impl Calendar {
    /// Occurrences qui recoupent `[from, to[`, triées par début. `local` : fuseau de qui
    /// regarde (journées entières, heures flottantes sans `X-WR-TIMEZONE`).
    pub fn occurrences(&self, from: Timestamp, to: Timestamp, local: &TimeZone) -> Vec<Occurrence> {
        let default = self.time_zone.clone().unwrap_or_else(|| local.clone());
        // Occurrences remplacées par une version modifiée (RECURRENCE-ID).
        let mut replaced: HashMap<&str, Vec<&IcsTime>> = HashMap::new();
        for e in &self.events {
            if let Some(rid) = &e.recurrence_id {
                replaced.entry(e.uid.as_str()).or_default().push(rid);
            }
        }
        let mut out = Vec::new();
        for e in &self.events {
            if e.cancelled {
                continue;
            }
            let skip = if e.recurrence_id.is_none() {
                replaced.get(e.uid.as_str()).map(Vec::as_slice)
            } else {
                None
            };
            expand(
                e,
                from,
                to,
                local,
                &default,
                skip.unwrap_or_default(),
                &mut out,
            );
        }
        out.sort_by(|a, b| {
            a.when
                .start(local)
                .cmp(&b.when.start(local))
                .then_with(|| b.when.is_all_day().cmp(&a.when.is_all_day()))
                .then_with(|| a.summary.cmp(&b.summary))
        });
        out
    }
}

/// Toutes les occurrences d'un événement dans la fenêtre.
fn expand(
    e: &Event,
    from: Timestamp,
    to: Timestamp,
    local: &TimeZone,
    default: &TimeZone,
    replaced: &[&IcsTime],
    out: &mut Vec<Occurrence>,
) {
    let length = e.length(default);
    // Occurrences exclues (EXDATE) ou remplacées (RECURRENCE-ID) : par instant, ou par
    // jour quand l'exclusion est une date seule.
    let mut excluded_ts: HashSet<Timestamp> = HashSet::new();
    let mut excluded_dates: HashSet<Date> = HashSet::new();
    for t in e.exdates.iter().chain(replaced.iter().copied()) {
        match t {
            IcsTime::Date(d) => {
                excluded_dates.insert(*d);
            }
            other => {
                if let Some(ts) = other.timestamp(default) {
                    excluded_ts.insert(ts);
                }
            }
        }
    }

    let make = |start: &IcsTime| -> Option<Occurrence> {
        let when = match (start, &length) {
            (IcsTime::Date(d), Length::Days(n)) => When::AllDay {
                start: *d,
                end: d.checked_add((*n).days()).ok()?,
            },
            (IcsTime::Date(d), _) => When::AllDay {
                start: *d,
                end: d.tomorrow().ok()?,
            },
            (t, len) => {
                let (dt, tz) = t.civil(default)?;
                let z = to_zoned(dt, &tz)?;
                let end = match len {
                    Length::Exact(d) => z.checked_add(*d).ok()?,
                    Length::Nominal(s) => z.checked_add(*s).ok()?,
                    Length::Days(n) => z.checked_add((*n).days()).ok()?,
                };
                When::Timed {
                    start: z.timestamp(),
                    end: end.timestamp(),
                }
            }
        };
        Some(Occurrence {
            uid: e.uid.clone(),
            summary: e.summary.clone(),
            location: e.location.clone(),
            when,
        })
    };
    let is_excluded = |t: &IcsTime| {
        t.date(default).is_some_and(|d| excluded_dates.contains(&d))
            || (!matches!(t, IcsTime::Date(_))
                && t.timestamp(default)
                    .is_some_and(|ts| excluded_ts.contains(&ts)))
    };
    // Ajoute l'occurrence si elle tombe dans la fenêtre ; `true` si ajoutée.
    let push = |t: &IcsTime, out: &mut Vec<Occurrence>| -> bool {
        if is_excluded(t) {
            return false;
        }
        match make(t) {
            Some(o) if o.when.overlaps(from, to, local) => {
                out.push(o);
                true
            }
            _ => false,
        }
    };

    for rdate in &e.rdates {
        push(rdate, out);
    }
    let Some(rule) = &e.rrule else {
        push(&e.start, out);
        return;
    };
    // Pour une règle, on travaille en heure civile dans le fuseau de l'événement :
    // un rendez-vous hebdomadaire à 9 h reste à 9 h après le changement d'heure.
    let (start_date, start_time, tz) = match &e.start {
        IcsTime::Date(d) => (*d, None, None),
        other => match other.civil(default) {
            Some((dt, tz)) => (dt.date(), Some(dt.time()), Some(tz)),
            None => return,
        },
    };
    let at = |d: Date| -> IcsTime {
        match (start_time, &tz) {
            (Some(time), Some(tz)) => IcsTime::Local(d.to_datetime(time), tz.clone()),
            _ => IcsTime::Date(d),
        }
    };
    let until = rule.until.as_ref().and_then(|u| match u {
        IcsTime::Date(d) => Some(Until::Date(*d)),
        IcsTime::Floating(dt) => tz
            .as_ref()
            .and_then(|tz| to_zoned(*dt, tz))
            .map(|z| Until::At(z.timestamp())),
        other => other.timestamp(default).map(Until::At),
    });
    // Dernier jour de la fenêtre : les périodes suivantes ne peuvent plus y tomber.
    let window_end_date = to.to_zoned_datetime(local.clone()).date();

    let mut seen = 0u32;
    let mut produced = 0usize;
    'periods: for i in 0..MAX_PERIODS {
        let Some((period_start, dates)) = candidates(rule, start_date, i) else {
            break;
        };
        if period_start > window_end_date {
            break;
        }
        for d in dates {
            if d < start_date {
                continue;
            }
            let t = at(d);
            match until {
                Some(Until::Date(u)) if d > u => break 'periods,
                Some(Until::At(u)) if t.timestamp(default).is_some_and(|ts| ts > u) => {
                    break 'periods;
                }
                _ => {}
            }
            seen += 1;
            if rule.count.is_some_and(|c| seen > c) {
                break 'periods;
            }
            if t.timestamp(local).is_some_and(|ts| ts >= to) {
                break 'periods;
            }
            if push(&t, out) {
                produced += 1;
            }
            if produced >= MAX_OCCURRENCES {
                log::debug!("récurrence coupée à {MAX_OCCURRENCES} occurrences");
                break 'periods;
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Until {
    Date(Date),
    At(Timestamp),
}

fn days_in_month(year: i16, month: i8) -> i8 {
    Date::new(year, month, 1).map_or(28, |d| d.days_in_month())
}

/// Jour `md` du mois (`-1` : le dernier).
fn month_day(year: i16, month: i8, md: i8) -> Option<Date> {
    let dim = days_in_month(year, month);
    let day = if md < 0 { dim + md + 1 } else { md };
    if day < 1 || day > dim {
        return None;
    }
    Date::new(year, month, day).ok()
}

/// Jours `wd` du mois, ou seulement le `n`-ième (`-1` : le dernier).
fn month_weekdays(year: i16, month: i8, n: i16, wd: Weekday) -> Vec<Date> {
    let Ok(first) = Date::new(year, month, 1) else {
        return Vec::new();
    };
    let offset = i64::from(
        (wd.to_monday_zero_offset() - first.weekday().to_monday_zero_offset()).rem_euclid(7),
    );
    let mut all = Vec::new();
    let mut d = first.checked_add(offset.days()).ok();
    while let Some(day) = d {
        if day.month() != month {
            break;
        }
        all.push(day);
        d = day.checked_add(7.days()).ok();
    }
    match n {
        0 => all,
        n if n > 0 => all
            .get(usize::try_from(n - 1).unwrap_or(usize::MAX))
            .copied()
            .into_iter()
            .collect(),
        n => {
            let back = usize::try_from(-n).unwrap_or(usize::MAX);
            all.len()
                .checked_sub(back)
                .and_then(|i| all.get(i).copied())
                .into_iter()
                .collect()
        }
    }
}

/// Jours d'un mois retenus par une règle mensuelle ou annuelle.
fn days_of_month(rule: &RRule, year: i16, month: i8, default_day: i8) -> Vec<Date> {
    let mut days: Vec<Date> = if !rule.by_day.is_empty() {
        let mut v: Vec<Date> = rule
            .by_day
            .iter()
            .flat_map(|(n, wd)| month_weekdays(year, month, *n, *wd))
            .collect();
        if !rule.by_month_day.is_empty() {
            let allowed: Vec<Date> = rule
                .by_month_day
                .iter()
                .filter_map(|md| month_day(year, month, *md))
                .collect();
            v.retain(|d| allowed.contains(d));
        }
        v
    } else if !rule.by_month_day.is_empty() {
        rule.by_month_day
            .iter()
            .filter_map(|md| month_day(year, month, *md))
            .collect()
    } else {
        // Le jour de DTSTART ; un 31 saute les mois plus courts (RFC 5545).
        Date::new(year, month, default_day)
            .ok()
            .into_iter()
            .collect()
    };
    days.sort();
    days.dedup();
    days
}

/// Dates candidates de la période `i` (et le premier jour de la période), triées,
/// `BYSETPOS` appliqué. `None` : hors des dates représentables.
fn candidates(rule: &RRule, start: Date, i: i64) -> Option<(Date, Vec<Date>)> {
    let step = i.checked_mul(i64::from(rule.interval))?;
    let in_months = |d: &Date| rule.by_month.is_empty() || rule.by_month.contains(&d.month());
    let (period_start, mut dates) = match rule.freq {
        Freq::Daily => {
            let d = start.checked_add(step.days()).ok()?;
            let keep = in_months(&d)
                && (rule.by_day.is_empty() || rule.by_day.iter().any(|(_, wd)| *wd == d.weekday()))
                && (rule.by_month_day.is_empty()
                    || rule
                        .by_month_day
                        .iter()
                        .any(|md| month_day(d.year(), d.month(), *md) == Some(d)));
            (d, if keep { vec![d] } else { Vec::new() })
        }
        Freq::Weekly => {
            let wkst = rule.week_start.to_monday_zero_offset();
            let back = i64::from((start.weekday().to_monday_zero_offset() - wkst).rem_euclid(7));
            let week = start
                .checked_sub(back.days())
                .ok()?
                .checked_add(step.checked_mul(7)?.days())
                .ok()?;
            let weekdays: Vec<Weekday> = if rule.by_day.is_empty() {
                vec![start.weekday()]
            } else {
                rule.by_day.iter().map(|(_, wd)| *wd).collect()
            };
            let mut v: Vec<Date> = weekdays
                .iter()
                .filter_map(|wd| {
                    let off = i64::from((wd.to_monday_zero_offset() - wkst).rem_euclid(7));
                    week.checked_add(off.days()).ok()
                })
                .filter(in_months)
                .collect();
            v.sort();
            v.dedup();
            (week, v)
        }
        Freq::Monthly => {
            let first = start.first_of_month().checked_add(step.months()).ok()?;
            let v = if in_months(&first) {
                days_of_month(rule, first.year(), first.month(), start.day())
            } else {
                Vec::new()
            };
            (first, v)
        }
        Freq::Yearly => {
            let year = i16::try_from(i64::from(start.year()).checked_add(step)?).ok()?;
            let first = Date::new(year, 1, 1).ok()?;
            let months: Vec<i8> = if rule.by_month.is_empty() {
                vec![start.month()]
            } else {
                let mut m = rule.by_month.clone();
                m.sort();
                m
            };
            let v = months
                .iter()
                .flat_map(|m| days_of_month(rule, year, *m, start.day()))
                .collect();
            (first, v)
        }
    };
    if !rule.by_set_pos.is_empty() && !dates.is_empty() {
        let len = i32::try_from(dates.len()).ok()?;
        let mut picked: Vec<Date> = rule
            .by_set_pos
            .iter()
            .filter_map(|p| {
                let idx = if *p > 0 { p - 1 } else { len + p };
                usize::try_from(idx)
                    .ok()
                    .and_then(|i| dates.get(i).copied())
            })
            .collect();
        picked.sort();
        picked.dedup();
        dates = picked;
    }
    Some((period_start, dates))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    const GOOGLE: &str = include_str!("../tests/data/google.ics");
    const OUTLOOK: &str = include_str!("../tests/data/outlook.ics");

    fn paris() -> TimeZone {
        TimeZone::get("Europe/Paris").unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn window(cal: &Calendar, from: &str, to: &str) -> Vec<Occurrence> {
        cal.occurrences(ts(from), ts(to), &paris())
    }

    fn starts(occ: &[Occurrence], summary: &str) -> Vec<String> {
        occ.iter()
            .filter(|o| o.summary == summary)
            .map(|o| match &o.when {
                When::AllDay { start, .. } => start.to_string(),
                When::Timed { start, .. } => {
                    start.to_zoned_datetime(paris()).datetime().to_string()
                }
            })
            .collect()
    }

    #[test]
    fn lines_are_unfolded_and_parsed() {
        let lines = unfold(
            "SUMMARY:Réunion\r\n  avec l'équipe\r\nDTSTART;TZID=\"Europe/Paris\":20261126T090000\r\n",
        );
        assert_eq!(lines[0], "SUMMARY:Réunion avec l'équipe");
        let p = parse_line(&lines[1]).unwrap();
        assert_eq!(p.name, "DTSTART");
        assert_eq!(p.param("TZID"), Some("Europe/Paris"));
        assert_eq!(p.value, "20261126T090000");
        let p = parse_line("ATTENDEE;CN=\"Doe; John\";ROLE=REQ:mailto:a@b.c").unwrap();
        assert_eq!(p.param("CN"), Some("Doe; John"));
        assert_eq!(p.value, "mailto:a@b.c");
        assert_eq!(
            unescape(r"Lyon\, salle 2\nétage\;B"),
            "Lyon, salle 2\nétage;B"
        );
    }

    #[test]
    fn time_zones_are_found_by_iana_prefix_or_windows_name() {
        assert!(resolve_time_zone("Europe/Paris").is_some());
        assert!(resolve_time_zone("/mozilla.org/20050126_1/Europe/Paris").is_some());
        let tz = resolve_time_zone("Romance Standard Time").unwrap();
        assert_eq!(tz.iana_name(), Some("Europe/Paris"));
        assert!(resolve_time_zone("Customized Time Zone").is_none());
    }

    #[test]
    fn not_a_calendar_is_refused() {
        assert!(parse("<html>").is_err());
    }

    #[test]
    fn google_calendar_name_and_single_events() {
        let cal = parse(GOOGLE).unwrap();
        assert_eq!(cal.name.as_deref(), Some("Perso"));
        let occ = window(&cal, "2026-11-01T00:00:00Z", "2026-12-01T00:00:00Z");
        // Heure UTC rendue à l'heure de Paris.
        assert_eq!(starts(&occ, "Dentiste"), ["2026-11-27T14:30:00"]);
        // Heure flottante : heure de l'agenda (X-WR-TIMEZONE).
        assert_eq!(starts(&occ, "Appel flottant"), ["2026-11-28T18:00:00"]);
        // Journée entière sur trois jours, `end` exclu.
        let trip = occ.iter().find(|o| o.summary == "Week-end à Lyon").unwrap();
        assert_eq!(
            trip.when,
            When::AllDay {
                start: date(2026, 11, 20),
                end: date(2026, 11, 23)
            }
        );
        assert!(trip.when.on_day(date(2026, 11, 22), &paris()));
        assert!(!trip.when.on_day(date(2026, 11, 23), &paris()));
        // Le rappel (VALARM) ne remplace pas la description de l'événement.
        assert_eq!(trip.location, "Lyon, France");
        // Événement annulé : absent.
        assert!(starts(&occ, "Annulé").is_empty());
    }

    #[test]
    fn weekly_rule_keeps_local_time_across_daylight_saving() {
        let cal = parse(GOOGLE).unwrap();
        let occ = window(&cal, "2026-10-19T00:00:00Z", "2026-11-10T00:00:00Z");
        // Lundi et mercredi à 9 h, avant et après le passage à l'heure d'hiver (25/10).
        assert_eq!(
            starts(&occ, "Sport"),
            [
                "2026-10-19T09:00:00",
                "2026-10-21T09:00:00",
                "2026-10-26T09:00:00",
                // Le 28/10 est exclu (EXDATE).
                "2026-11-02T09:00:00",
                "2026-11-04T09:00:00",
                "2026-11-09T09:00:00",
            ]
        );
    }

    #[test]
    fn daily_rule_with_count_and_moved_occurrence() {
        let cal = parse(GOOGLE).unwrap();
        let occ = window(&cal, "2026-11-01T00:00:00Z", "2026-12-31T00:00:00Z");
        // 5 occurrences ; la troisième est déplacée à 15 h (RECURRENCE-ID).
        assert_eq!(
            starts(&occ, "Médicament"),
            [
                "2026-11-16T08:00:00",
                "2026-11-17T08:00:00",
                "2026-11-19T08:00:00",
                "2026-11-20T08:00:00",
            ]
        );
        assert_eq!(starts(&occ, "Médicament (décalé)"), ["2026-11-18T15:00:00"]);
    }

    #[test]
    fn monthly_rules_by_weekday_rank_and_month_day() {
        let cal = parse(GOOGLE).unwrap();
        let occ = window(&cal, "2026-11-01T00:00:00Z", "2027-03-01T00:00:00Z");
        // Dernier vendredi du mois, jusqu'au 31/01/2027.
        assert_eq!(
            starts(&occ, "Apéro de fin de mois"),
            [
                "2026-11-27T19:00:00",
                "2026-12-25T19:00:00",
                "2027-01-29T19:00:00",
            ]
        );
        // Le 31 : les mois plus courts sont sautés.
        assert_eq!(starts(&occ, "Loyer"), ["2026-12-31", "2027-01-31"]);
        // Tous les 2 mois, deuxième mardi.
        assert_eq!(
            starts(&occ, "Comité"),
            ["2026-11-10T18:30:00", "2027-01-12T18:30:00"]
        );
    }

    #[test]
    fn yearly_birthday_and_set_position() {
        let cal = parse(GOOGLE).unwrap();
        let occ = window(&cal, "2026-01-01T00:00:00Z", "2028-01-01T00:00:00Z");
        assert_eq!(
            starts(&occ, "Anniversaire de Léa"),
            ["2026-03-14", "2027-03-14"]
        );
        // Dernier jour ouvré du mois (BYSETPOS=-1), 3 fois.
        assert_eq!(
            starts(&occ, "Notes de frais"),
            [
                "2026-11-30T17:00:00",
                "2026-12-31T17:00:00",
                "2027-01-29T17:00:00",
            ]
        );
    }

    #[test]
    fn outlook_windows_time_zones_and_durations() {
        let cal = parse(OUTLOOK).unwrap();
        let occ = window(&cal, "2026-11-01T00:00:00Z", "2026-12-01T00:00:00Z");
        // TZID Windows « Romance Standard Time » = Europe/Paris.
        assert_eq!(starts(&occ, "Point d'équipe"), ["2026-11-26T10:00:00"]);
        let point = occ.iter().find(|o| o.summary == "Point d'équipe").unwrap();
        assert_eq!(
            point
                .when
                .end(&paris())
                .duration_since(point.when.start(&paris())),
            SignedDuration::from_mins(45)
        );
        // Heure de New York (Eastern Standard Time) : 9 h là-bas = 15 h à Paris.
        assert_eq!(starts(&occ, "Call New York"), ["2026-11-24T15:00:00"]);
        // DURATION au lieu de DTEND.
        let call = occ.iter().find(|o| o.summary == "Call New York").unwrap();
        assert_eq!(
            call.when
                .end(&paris())
                .duration_since(call.when.start(&paris())),
            SignedDuration::from_mins(90)
        );
        // Hebdomadaire toutes les 2 semaines, mardi et jeudi, jusqu'au 20/11 (UNTIL UTC).
        assert_eq!(
            starts(&occ, "Revue de code"),
            [
                "2026-11-03T14:00:00",
                "2026-11-05T14:00:00",
                "2026-11-17T14:00:00",
                "2026-11-19T14:00:00",
            ]
        );
    }

    #[test]
    fn occurrences_are_sorted_and_windowed() {
        let cal = parse(GOOGLE).unwrap();
        let occ = window(&cal, "2026-11-16T00:00:00Z", "2026-11-17T00:00:00Z");
        let names: Vec<&str> = occ.iter().map(|o| o.summary.as_str()).collect();
        assert_eq!(names, ["Médicament", "Sport"]);
    }

    #[test]
    fn endless_daily_rule_is_bounded() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:x\nSUMMARY:Tous les jours\nDTSTART:19900101T080000Z\nRRULE:FREQ=DAILY\nEND:VEVENT\nEND:VCALENDAR\n";
        let cal = parse(text).unwrap();
        let occ = window(&cal, "2026-11-01T00:00:00Z", "2026-11-08T00:00:00Z");
        assert_eq!(occ.len(), 7);
    }
}
