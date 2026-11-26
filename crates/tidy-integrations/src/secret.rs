//! Secrets (clé de l'API Claude, adresses privées des agendas ICS) dans le gestionnaire d'identifiants du système :
//! Gestionnaire d'identifiants de Windows, trousseau de macOS (crate `keyring`).
//!
//! Une clé ne s'écrit jamais dans `config.json`, dans les réglages d'un widget ni dans
//! un journal.

use std::fmt;

/// Erreur du gestionnaire d'identifiants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretError {
    /// Gestionnaire d'identifiants absent ou refusé (accès au trousseau refusé…).
    Unavailable(String),
}

impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretError::Unavailable(why) => {
                write!(f, "gestionnaire d'identifiants indisponible : {why}")
            }
        }
    }
}

impl std::error::Error for SecretError {}

/// Un secret rangé quelque part.
pub trait SecretStore: Send + Sync {
    /// Le secret, ou `None` s'il n'y en a pas.
    fn get(&self) -> Result<Option<String>, SecretError>;
    fn set(&self, value: &str) -> Result<(), SecretError>;
    /// Efface le secret (sans erreur s'il n'y en avait pas).
    fn delete(&self) -> Result<(), SecretError>;
}

/// Entrée du gestionnaire d'identifiants du système.
#[derive(Debug, Clone, Copy)]
pub struct Keyring {
    pub service: &'static str,
    pub account: &'static str,
}

/// Clé de l'API Claude (« Tidy » / « anthropic-api-key »).
pub const CLAUDE_API_KEY: Keyring = Keyring {
    service: "Tidy",
    account: "anthropic-api-key",
};

impl Keyring {
    fn entry(&self) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(self.service, self.account).map_err(unavailable)
    }
}

/// Entrée du gestionnaire d'identifiants dont le nom n'est connu qu'à l'exécution
/// (adresse privée d'un agenda ICS : « Tidy » / « ics-<identifiant de l'agenda> »).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedKeyring {
    pub service: &'static str,
    pub account: String,
}

impl NamedKeyring {
    /// Adresse ICS de l'agenda `id`.
    pub fn ics_calendar(id: &str) -> Self {
        Self {
            service: "Tidy",
            account: format!("ics-{id}"),
        }
    }

    fn entry(&self) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(self.service, &self.account).map_err(unavailable)
    }
}

impl SecretStore for NamedKeyring {
    fn get(&self) -> Result<Option<String>, SecretError> {
        get(self.entry()?)
    }

    fn set(&self, value: &str) -> Result<(), SecretError> {
        self.entry()?.set_password(value).map_err(unavailable)
    }

    fn delete(&self) -> Result<(), SecretError> {
        delete(self.entry()?)
    }
}

fn get(entry: keyring::Entry) -> Result<Option<String>, SecretError> {
    match entry.get_password() {
        Ok(v) if v.trim().is_empty() => Ok(None),
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(unavailable(e)),
    }
}

fn delete(entry: keyring::Entry) -> Result<(), SecretError> {
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(unavailable(e)),
    }
}

/// Erreur de `keyring`, sans le secret (les erreurs de `keyring` n'en contiennent pas,
/// sauf `BadEncoding`, réduite ici à son nom).
fn unavailable(e: keyring::Error) -> SecretError {
    let why = match e {
        keyring::Error::BadEncoding(_) => "secret illisible".to_owned(),
        other => other.to_string(),
    };
    SecretError::Unavailable(why)
}

impl SecretStore for Keyring {
    fn get(&self) -> Result<Option<String>, SecretError> {
        get(self.entry()?)
    }

    fn set(&self, value: &str) -> Result<(), SecretError> {
        self.entry()?.set_password(value).map_err(unavailable)
    }

    fn delete(&self) -> Result<(), SecretError> {
        delete(self.entry()?)
    }
}

/// Secret en mémoire (tests, aperçu).
#[derive(Debug, Default)]
pub struct MemorySecret(std::sync::Mutex<Option<String>>);

impl SecretStore for MemorySecret {
    fn get(&self) -> Result<Option<String>, SecretError> {
        Ok(self.0.lock().map_err(|_| poisoned())?.clone())
    }

    fn set(&self, value: &str) -> Result<(), SecretError> {
        *self.0.lock().map_err(|_| poisoned())? = Some(value.to_owned());
        Ok(())
    }

    fn delete(&self) -> Result<(), SecretError> {
        *self.0.lock().map_err(|_| poisoned())? = None;
        Ok(())
    }
}

fn poisoned() -> SecretError {
    SecretError::Unavailable("verrou empoisonné".to_owned())
}

/// Forme plausible d'une clé de l'API Claude (`sk-ant-…`), pour prévenir d'une faute
/// de copie avant le premier envoi.
pub fn looks_like_claude_key(key: &str) -> bool {
    let key = key.trim();
    key.starts_with("sk-ant-") && key.len() > 20 && !key.contains(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_secret_round_trip() {
        let s = MemorySecret::default();
        assert_eq!(s.get().unwrap(), None);
        s.set("sk-ant-essai").unwrap();
        assert_eq!(s.get().unwrap().as_deref(), Some("sk-ant-essai"));
        s.delete().unwrap();
        assert_eq!(s.get().unwrap(), None);
    }

    #[test]
    fn claude_key_shape() {
        assert!(looks_like_claude_key(
            "sk-ant-api03-abcdefghijklmnopqrstuvwxyz"
        ));
        assert!(!looks_like_claude_key("sk-ant-api03 abc def ghi jkl mno"));
        assert!(!looks_like_claude_key("bonjour"));
    }
}
