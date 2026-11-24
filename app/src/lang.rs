//! Choix de la langue de l'interface d'après les réglages et le système.

use tidy_core::settings::Language;
use tidy_ui::Lang;

/// Langue à utiliser : celle des réglages, ou la première langue préférée du système
/// que Tidy connaît, sinon le français.
pub fn resolve(setting: Language) -> Lang {
    match setting {
        Language::Fr => Lang::Fr,
        Language::En => Lang::En,
        Language::System => tidy_platform::current()
            .preferred_languages()
            .iter()
            .find_map(|code| Lang::from_code(code))
            .unwrap_or(Lang::Fr),
    }
}

/// Applique la langue des réglages.
pub fn apply(setting: Language) {
    tidy_ui::set_language(resolve(setting));
}
