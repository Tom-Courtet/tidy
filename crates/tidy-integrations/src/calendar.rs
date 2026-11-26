//! Agendas : trait [`CalendarProvider`] et lecture d'une URL ICS (ADR 008, « À revoir
//! pour Windows » : l'URL ICS d'abord, Google et Outlook en donnent une).
//!
//! EventKit (macOS) et l'OAuth Google ou Microsoft Graph viendront derrière le même
//! trait, s'il le faut.

use std::fmt;

use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::ics::{self, Occurrence};
use crate::net::{self, NetError};

/// Erreur de chargement d'un agenda.
#[derive(Debug, Clone, PartialEq)]
pub enum CalendarError {
    Net(NetError),
    /// Adresse vide ou qui n'est pas une adresse web.
    BadUrl,
    /// La réponse n'est pas un agenda iCalendar (page de connexion, adresse publique
    /// au lieu de l'adresse secrète…).
    NotACalendar,
}

impl fmt::Display for CalendarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CalendarError::Net(e) => e.fmt(f),
            CalendarError::BadUrl => write!(f, "adresse d'agenda refusée"),
            CalendarError::NotACalendar => write!(f, "pas un agenda iCalendar"),
        }
    }
}

impl std::error::Error for CalendarError {}

impl From<NetError> for CalendarError {
    fn from(e: NetError) -> Self {
        match e {
            NetError::BadUrl => CalendarError::BadUrl,
            e => CalendarError::Net(e),
        }
    }
}

/// Agenda chargé, interrogé ensuite sans réseau (changement de mois…).
pub trait Agenda: Send {
    /// Nom donné par le service, s'il y en a un.
    fn name(&self) -> Option<String>;
    /// Occurrences qui recoupent `[from, to[`, triées par début.
    fn occurrences(&self, from: Timestamp, to: Timestamp, local: &TimeZone) -> Vec<Occurrence>;
}

impl Agenda for ics::Calendar {
    fn name(&self) -> Option<String> {
        self.name.clone()
    }

    fn occurrences(&self, from: Timestamp, to: Timestamp, local: &TimeZone) -> Vec<Occurrence> {
        ics::Calendar::occurrences(self, from, to, local)
    }
}

/// Fournisseur d'agenda.
pub trait CalendarProvider: Send + Sync {
    /// Charge l'agenda (réseau : à appeler hors du fil de l'interface).
    fn load(&self) -> Result<Box<dyn Agenda>, CalendarError>;
}

/// Agenda publié à une adresse ICS (`https://…` ou `webcal://…`).
#[derive(Clone)]
pub struct IcsUrl {
    url: String,
}

impl fmt::Debug for IcsUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // L'adresse secrète d'un agenda donne accès à son contenu : seul l'hôte apparaît.
        f.debug_struct("IcsUrl")
            .field("host", &net::host_of(&self.url))
            .finish()
    }
}

impl IcsUrl {
    /// Adresse vérifiée (`webcal://` devient `https://`).
    pub fn new(url: &str) -> Result<Self, CalendarError> {
        normalize_url(url).map(|url| Self { url })
    }
}

/// Adresse utilisable : `webcal://` → `https://`, espaces retirées, http(s) seulement.
pub fn normalize_url(url: &str) -> Result<String, CalendarError> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    let url = if let Some(rest) = lower.strip_prefix("webcal://") {
        format!("https://{}", &url[url.len() - rest.len()..])
    } else if lower.starts_with("https://") || lower.starts_with("http://") {
        url.to_owned()
    } else {
        return Err(CalendarError::BadUrl);
    };
    if net::host_of(&url).is_empty() {
        return Err(CalendarError::BadUrl);
    }
    Ok(url)
}

impl CalendarProvider for IcsUrl {
    fn load(&self) -> Result<Box<dyn Agenda>, CalendarError> {
        let text = net::get_text(&self.url, &[]).inspect_err(|e| {
            log::warn!("agenda {} : {e}", net::host_of(&self.url));
        })?;
        let calendar = ics::parse(&text).map_err(|_| CalendarError::NotACalendar)?;
        log::debug!(
            "agenda {} : {} événements",
            net::host_of(&self.url),
            calendar.events.len()
        );
        Ok(Box::new(calendar))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webcal_becomes_https_and_other_schemes_are_refused() {
        assert_eq!(
            normalize_url(" webcal://p01-caldav.icloud.com/published/2/abc ").unwrap(),
            "https://p01-caldav.icloud.com/published/2/abc"
        );
        assert_eq!(
            normalize_url("https://calendar.google.com/x/basic.ics").unwrap(),
            "https://calendar.google.com/x/basic.ics"
        );
        assert_eq!(
            normalize_url("file:///etc/passwd"),
            Err(CalendarError::BadUrl)
        );
        assert_eq!(normalize_url(""), Err(CalendarError::BadUrl));
        assert_eq!(normalize_url("https://"), Err(CalendarError::BadUrl));
    }

    #[test]
    fn debug_hides_the_secret_address() {
        let url =
            IcsUrl::new("https://calendar.google.com/calendar/ical/private-123/basic.ics").unwrap();
        let shown = format!("{url:?}");
        assert!(shown.contains("calendar.google.com"));
        assert!(!shown.contains("private-123"));
    }
}
