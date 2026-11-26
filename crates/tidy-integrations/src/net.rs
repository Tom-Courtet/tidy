//! Accès HTTP communs (ureq, TLS du système, délais bornés) et erreurs réseau.

use std::fmt;
use std::time::Duration;

/// Identité envoyée aux services (sans rien sur l'utilisateur).
pub const USER_AGENT: &str = concat!("Tidy/", env!("CARGO_PKG_VERSION"));

/// Taille maximale d'une réponse lue d'un coup (un agenda ICS chargé fait quelques Mo).
const MAX_BODY: u64 = 20 * 1024 * 1024;

/// Erreur réseau, sans texte d'interface : le widget la traduit.
#[derive(Debug, Clone, PartialEq)]
pub enum NetError {
    /// Pas de connexion, ou serveur introuvable.
    Offline,
    /// Le serveur ne répond pas à temps.
    Timeout,
    /// Réponse HTTP en erreur (404, 500…).
    Status(u16),
    /// Connexion sécurisée refusée (certificat).
    Tls,
    /// Adresse refusée (mal formée, protocole inconnu).
    BadUrl,
    /// Réponse reçue mais illisible.
    Invalid(String),
    /// Autre erreur, pour le journal.
    Other(String),
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::Offline => write!(f, "hors ligne ou serveur introuvable"),
            NetError::Timeout => write!(f, "délai dépassé"),
            NetError::Status(code) => write!(f, "réponse HTTP {code}"),
            NetError::Tls => write!(f, "connexion sécurisée impossible"),
            NetError::BadUrl => write!(f, "adresse refusée"),
            NetError::Invalid(why) => write!(f, "réponse illisible : {why}"),
            NetError::Other(why) => write!(f, "erreur réseau : {why}"),
        }
    }
}

impl std::error::Error for NetError {}

impl From<ureq::Error> for NetError {
    fn from(e: ureq::Error) -> Self {
        use ureq::Error;
        match e {
            Error::StatusCode(code) => NetError::Status(code),
            Error::Timeout(_) => NetError::Timeout,
            Error::HostNotFound | Error::ConnectionFailed => NetError::Offline,
            Error::Tls(_) | Error::NativeTls(_) | Error::Der(_) | Error::TlsRequired => {
                NetError::Tls
            }
            Error::BadUri(_) => NetError::BadUrl,
            Error::Io(e) => io_error(&e),
            other => NetError::Other(other.to_string()),
        }
    }
}

/// Erreur d'entrée-sortie pendant un échange.
pub fn io_error(e: &std::io::Error) -> NetError {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => NetError::Timeout,
        ErrorKind::ConnectionRefused
        | ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::NotConnected
        | ErrorKind::BrokenPipe
        | ErrorKind::UnexpectedEof => NetError::Offline,
        _ => NetError::Other(e.to_string()),
    }
}

/// Réglages d'un client HTTP.
pub struct AgentOptions {
    /// Durée maximale d'un échange complet (`None` : sans limite, pour un flux).
    pub total: Option<Duration>,
    /// Les réponses 4xx et 5xx sont rendues telles quelles (corps d'erreur lisible).
    pub status_as_response: bool,
}

impl Default for AgentOptions {
    fn default() -> Self {
        Self {
            total: Some(Duration::from_secs(30)),
            status_as_response: false,
        }
    }
}

/// Client HTTP : TLS du système (schannel, Security.framework), délais bornés.
pub fn agent(options: AgentOptions) -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
    let tls = TlsConfig::builder()
        .provider(TlsProvider::NativeTls)
        .root_certs(RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_global(options.total)
        .max_redirects(5)
        .http_status_as_error(!options.status_as_response)
        .user_agent(USER_AGENT)
        .build()
        .into()
}

/// GET qui rend le corps en texte. `query` est encodé par ureq.
pub fn get_text(url: &str, query: &[(&str, &str)]) -> Result<String, NetError> {
    let mut request = agent(AgentOptions::default()).get(url);
    for (name, value) in query {
        request = request.query(*name, *value);
    }
    let mut response = request.call()?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_string()
        .map_err(NetError::from)
}

/// Hôte d'une adresse, pour les journaux (une URL d'agenda privée ne s'y écrit jamais
/// en entier).
pub fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..end];
    host.rsplit_once('@').map_or(host, |(_, h)| h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_hides_the_private_part_of_an_url() {
        assert_eq!(
            host_of(
                "https://calendar.google.com/calendar/ical/x%40gmail.com/private-abc/basic.ics"
            ),
            "calendar.google.com"
        );
        assert_eq!(host_of("webcal://user:pw@example.org?x"), "example.org");
        assert_eq!(host_of("outlook.office365.com"), "outlook.office365.com");
    }

    #[test]
    fn io_errors_are_classified() {
        let e = std::io::Error::from(std::io::ErrorKind::TimedOut);
        assert_eq!(io_error(&e), NetError::Timeout);
        let e = std::io::Error::from(std::io::ErrorKind::ConnectionReset);
        assert_eq!(io_error(&e), NetError::Offline);
    }
}
