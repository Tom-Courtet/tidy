//! Boîte privée de Tidy : un coffre chiffré **fichier par fichier** (ADR 013), le même
//! sous Windows et sous macOS.
//!
//! - Une clé maîtresse aléatoire de 32 octets chiffre chaque fichier
//!   (XChaCha20-Poly1305, par morceaux d'un Mio) et l'index (noms, tailles, dates).
//! - Elle est enveloppée deux fois, par Argon2id : par le mot de passe (6 à 20
//!   caractères) et par une [`RecoveryKey`] affichée une seule fois.
//! - Ouvrir un fichier le déchiffre dans un dossier temporaire du coffre ; ses
//!   modifications sont rechiffrées au verrouillage, puis la copie est écrasée et
//!   supprimée.
//!
//! **Aucun fichier de l'utilisateur n'est perdu** : un ajout n'appelle `dispose` (la
//! corbeille) qu'une fois le fichier chiffré, relu, entièrement déchiffré et comparé à
//! l'original, puis l'index écrit ; un échec à n'importe quelle étape laisse
//! l'original en place. Une extraction n'efface la copie chiffrée qu'une fois le
//! fichier en clair écrit à sa place définitive.
//!
//! Mots de passe et clés ne sont jamais journalisés ; ils sont effacés de la mémoire
//! (`zeroize`) après usage.

mod crypto;
mod file_vault;
mod fsx;
pub mod recovery;

use std::fmt;
use std::path::{Path, PathBuf};

pub use crypto::KdfParams;
pub use file_vault::FileVault;
pub use recovery::RecoveryKey;
pub use zeroize::{Zeroize, Zeroizing};

/// Un secret saisi (mot de passe, clé de récupération), effacé à sa libération.
pub type Secret = Zeroizing<String>;

/// Longueur permise d'un mot de passe, en caractères.
pub const PASSWORD_MIN: usize = 6;
pub const PASSWORD_MAX: usize = 20;

/// Vérifie la longueur d'un mot de passe (6 à 20 caractères).
pub fn check_password(password: &str) -> Result<()> {
    let n = password.chars().count();
    if (PASSWORD_MIN..=PASSWORD_MAX).contains(&n) {
        Ok(())
    } else {
        Err(VaultError::BadPassword)
    }
}

/// Un élément de premier niveau du coffre (fichier ou dossier).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultItem {
    pub name: String,
    pub is_dir: bool,
    /// Taille en clair (somme des fichiers pour un dossier).
    pub size: u64,
    /// Date de modification d'origine, en secondes depuis 1970.
    pub modified: Option<i64>,
}

/// Bilan d'un verrouillage.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LockReport {
    /// Fichiers ouverts dont les modifications ont été rechiffrées.
    pub updated: usize,
    /// Copies temporaires laissées en place (modification non rechiffrée, ou fichier
    /// encore ouvert par une appli). Elles seront reprises au prochain déverrouillage.
    pub kept: Vec<PathBuf>,
}

/// Mise à la corbeille de l'original (ou de la copie extraite), fournie par l'appelant
/// (`DesktopPlatform::trash`). Jamais de suppression définitive.
pub type Dispose<'a> = &'a mut dyn FnMut(&Path) -> std::io::Result<()>;

/// Erreurs du coffre. Les messages sont en français, sans aucun secret.
#[derive(Debug)]
pub enum VaultError {
    /// Pas encore de coffre.
    NotCreated,
    /// Un coffre existe déjà.
    AlreadyExists,
    /// Opération impossible coffre verrouillé.
    Locked,
    WrongPassword,
    WrongRecoveryKey,
    /// Mot de passe trop court ou trop long.
    BadPassword,
    /// Clé de récupération mal formée.
    BadRecoveryKey,
    /// Données du coffre illisibles ou altérées.
    Corrupt(String),
    /// La relecture d'un fichier chiffré ne redonne pas l'original.
    Verification(String),
    NotFound(String),
    /// Nom refusé (vide, caractères interdits).
    BadName(String),
    /// Nom déjà pris dans le coffre.
    NameTaken(String),
    /// Élément non pris en charge (fichier trop gros, coffre d'une version plus
    /// récente).
    Unsupported(String),
    /// Lien symbolique (ou dossier qui en contient) : refusé, l'original reste.
    Symlink,
    /// Des copies ouvertes n'ont pas pu être effacées (fichier encore ouvert par une
    /// appli) : le coffre, vidé, n'est pas supprimé.
    StillOpen,
    /// L'original n'a pas pu aller à la corbeille : rien n'a changé.
    Disposal(String),
    Random(String),
    Io {
        context: &'static str,
        error: std::io::Error,
    },
}

impl VaultError {
    /// Fabrique d'erreurs d'entrée-sortie avec leur contexte.
    pub(crate) fn io(context: &'static str) -> impl Fn(std::io::Error) -> Self {
        move |error| Self::Io { context, error }
    }
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotCreated => f.write_str("pas de coffre"),
            Self::AlreadyExists => f.write_str("un coffre existe déjà"),
            Self::Locked => f.write_str("coffre verrouillé"),
            Self::WrongPassword => f.write_str("mot de passe incorrect"),
            Self::WrongRecoveryKey => f.write_str("clé de récupération incorrecte"),
            Self::BadPassword => write!(
                f,
                "le mot de passe doit faire {PASSWORD_MIN} à {PASSWORD_MAX} caractères"
            ),
            Self::BadRecoveryKey => f.write_str("clé de récupération mal formée"),
            Self::Corrupt(what) => write!(f, "coffre illisible : {what}"),
            Self::Verification(what) => write!(f, "vérification échouée : {what}"),
            Self::NotFound(name) => write!(f, "« {name} » n'est pas dans le coffre"),
            Self::BadName(name) => write!(f, "nom refusé : « {name} »"),
            Self::NameTaken(name) => write!(f, "« {name} » existe déjà dans le coffre"),
            Self::Unsupported(what) => write!(f, "non pris en charge : {what}"),
            Self::Symlink => f.write_str("les liens symboliques ne sont pas pris en charge"),
            Self::StillOpen => f.write_str("des fichiers du coffre sont encore ouverts"),
            Self::Disposal(e) => write!(f, "corbeille impossible : {e}"),
            Self::Random(e) => write!(f, "aléa du système indisponible : {e}"),
            Self::Io { context, error } => write!(f, "{context} : {error}"),
        }
    }
}

impl std::error::Error for VaultError {}

pub type Result<T> = std::result::Result<T, VaultError>;

/// Un coffre. [`FileVault`] est l'implémentation (Windows et macOS) ; le trait permet
/// d'en essayer une autre sans toucher à l'interface.
pub trait VaultBackend: Send {
    /// Un coffre existe sur le disque.
    fn exists(&self) -> bool;
    fn is_unlocked(&self) -> bool;
    /// Crée le coffre (vide) et le laisse déverrouillé.
    fn create(&mut self, password: &str, recovery: &RecoveryKey) -> Result<()>;
    /// Déverrouille. Reprend au passage les copies temporaires laissées par une session
    /// interrompue (leurs modifications sont rechiffrées).
    fn unlock(&mut self, password: &str) -> Result<()>;
    /// Déverrouille par la clé de récupération et remplace le mot de passe oublié.
    fn recover(&mut self, recovery: &str, new_password: &str) -> Result<()>;
    /// Rechiffre les fichiers ouverts modifiés, efface les copies temporaires et oublie
    /// la clé. Ne peut pas échouer : ce qui n'a pas pu être rechiffré reste en place
    /// (voir [`LockReport::kept`]).
    fn lock(&mut self) -> LockReport;
    /// Éléments de premier niveau.
    fn list(&self) -> Result<Vec<VaultItem>>;
    /// Ajoute un fichier ou un dossier, vérifie, puis appelle `dispose` sur l'original.
    /// Rend le nom pris dans le coffre.
    fn add(&mut self, source: &Path, dispose: Dispose) -> Result<String>;
    /// Remet un élément en clair dans `dest_dir` et le retire du coffre. Rend son
    /// chemin.
    fn extract(&mut self, name: &str, dest_dir: &Path) -> Result<PathBuf>;
    /// Déchiffre un élément dans le dossier temporaire et rend son chemin (à ouvrir
    /// avec l'appli par défaut).
    fn open(&mut self, name: &str) -> Result<PathBuf>;
    /// Met un élément à la corbeille (en clair) et le retire du coffre.
    fn trash(&mut self, name: &str, dispose: Dispose) -> Result<()>;
    /// Renomme un élément de premier niveau. Rend le nouveau nom.
    fn rename(&mut self, old: &str, new: &str) -> Result<String>;
    fn change_password(&mut self, old: &str, new: &str) -> Result<()>;
    /// Remet tous les éléments dans `dest_dir`, puis supprime le coffre (vide). Rend
    /// les chemins des éléments rendus.
    fn destroy(&mut self, password: &str, dest_dir: &Path) -> Result<Vec<PathBuf>>;
}
