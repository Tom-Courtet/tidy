//! Pause intelligente : faut-il arrêter la vidéo (et la playlist) maintenant ?

use serde::{Deserialize, Serialize};
use tidy_core::settings::wallpaper::PauseSettings;

/// État du système, relevé par l'agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Conditions {
    /// Une appli occupe tout un écran (jeu, vidéo, présentation).
    pub fullscreen: bool,
    /// Sur batterie (et non sur secteur).
    pub on_battery: bool,
    /// Charge de la batterie, en %, si connue.
    pub battery_percent: Option<u8>,
    pub locked: bool,
    /// Bureau masqué par Tidy (double-clic, raccourci).
    pub desktop_hidden: bool,
}

/// Pourquoi la vidéo est en pause (la première raison qui s'applique).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseReason {
    Locked,
    Fullscreen,
    Battery,
    DesktopHidden,
}

/// Raison de mettre en pause, ou `None` pour jouer.
pub fn reason(settings: &PauseSettings, c: &Conditions) -> Option<PauseReason> {
    if settings.when_locked && c.locked {
        return Some(PauseReason::Locked);
    }
    if settings.on_fullscreen && c.fullscreen {
        return Some(PauseReason::Fullscreen);
    }
    if settings.on_battery && c.on_battery {
        let threshold = settings.battery_threshold.min(100);
        // Charge inconnue : on suppose qu'elle est sous le seuil (prudence).
        if threshold >= 100 || c.battery_percent.is_none_or(|p| p <= threshold) {
            return Some(PauseReason::Battery);
        }
    }
    if settings.when_desktop_hidden && c.desktop_hidden {
        return Some(PauseReason::DesktopHidden);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_happening_means_play() {
        assert_eq!(
            reason(&PauseSettings::default(), &Conditions::default()),
            None
        );
    }

    #[test]
    fn each_condition_pauses_when_enabled() {
        let s = PauseSettings::default();
        let fullscreen = Conditions {
            fullscreen: true,
            ..Default::default()
        };
        assert_eq!(reason(&s, &fullscreen), Some(PauseReason::Fullscreen));
        let locked = Conditions {
            locked: true,
            fullscreen: true,
            ..Default::default()
        };
        assert_eq!(reason(&s, &locked), Some(PauseReason::Locked));
        let hidden = Conditions {
            desktop_hidden: true,
            ..Default::default()
        };
        assert_eq!(reason(&s, &hidden), Some(PauseReason::DesktopHidden));

        let off = PauseSettings {
            on_fullscreen: false,
            when_locked: false,
            when_desktop_hidden: false,
            ..Default::default()
        };
        assert_eq!(reason(&off, &locked), None);
        assert_eq!(reason(&off, &hidden), None);
    }

    #[test]
    fn battery_threshold() {
        let battery = |percent| Conditions {
            on_battery: true,
            battery_percent: percent,
            ..Default::default()
        };
        // 100 % : toujours en pause sur batterie.
        let always = PauseSettings::default();
        assert_eq!(
            reason(&always, &battery(Some(95))),
            Some(PauseReason::Battery)
        );
        let below_30 = PauseSettings {
            battery_threshold: 30,
            ..Default::default()
        };
        assert_eq!(reason(&below_30, &battery(Some(80))), None);
        assert_eq!(
            reason(&below_30, &battery(Some(30))),
            Some(PauseReason::Battery)
        );
        assert_eq!(
            reason(&below_30, &battery(None)),
            Some(PauseReason::Battery)
        );
        // Sur secteur : jamais pour la batterie.
        let plugged = Conditions {
            on_battery: false,
            battery_percent: Some(5),
            ..Default::default()
        };
        assert_eq!(reason(&below_30, &plugged), None);
        let disabled = PauseSettings {
            on_battery: false,
            ..Default::default()
        };
        assert_eq!(reason(&disabled, &battery(Some(1))), None);
    }
}
