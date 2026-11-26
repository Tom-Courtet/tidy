//! Boîte privée (lot F) : verrouillage automatique, opacité…
//!
//! Fichier propre à son lot : les champs s'ajoutent ici, chacun avec une valeur par
//! défaut (`#[serde(default)]` sur la structure). Ni mot de passe ni clé ici : ils ne
//! sont jamais écrits dans `config.json`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VaultSettings {
    /// Verrouiller après [`Self::auto_lock_minutes`] sans clavier ni souris.
    pub auto_lock: bool,
    /// Délai d'inactivité, en minutes (1 à 240).
    pub auto_lock_minutes: u32,
    /// Verrouiller à la mise en veille.
    pub lock_on_sleep: bool,
    /// Verrouiller quand la session est verrouillée (Win+L, écran de verrouillage).
    pub lock_on_session_lock: bool,
    /// Opacité de la boîte privée verrouillée, de 0 (invisible, elle revient au
    /// survol) à 1.
    pub locked_opacity: f32,
}

impl Default for VaultSettings {
    fn default() -> Self {
        Self {
            auto_lock: true,
            auto_lock_minutes: 10,
            lock_on_sleep: true,
            lock_on_session_lock: true,
            locked_opacity: 1.0,
        }
    }
}

impl VaultSettings {
    pub const MINUTES: std::ops::RangeInclusive<u32> = 1..=240;

    /// Délai d'inactivité borné, en secondes.
    pub fn idle_limit_secs(&self) -> f64 {
        f64::from(
            self.auto_lock_minutes
                .clamp(*Self::MINUTES.start(), *Self::MINUTES.end()),
        ) * 60.0
    }

    /// Opacité bornée à [0, 1].
    pub fn opacity(&self) -> f32 {
        if self.locked_opacity.is_finite() {
            self.locked_opacity.clamp(0.0, 1.0)
        } else {
            1.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_files_get_defaults_and_values_are_bounded() {
        let s: VaultSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, VaultSettings::default());
        let s: VaultSettings =
            serde_json::from_str(r#"{"auto_lock_minutes": 0, "locked_opacity": 3.0}"#).unwrap();
        assert_eq!(s.idle_limit_secs(), 60.0);
        assert_eq!(s.opacity(), 1.0);
        assert!(s.lock_on_sleep);
    }
}
