//! Interface commune à toutes les fenêtres de Tidy : thème (repris de Snappy, Tappy et
//! Soundy), polices, icônes Lucide, petits widgets et traductions.
//!
//! Règle : aucune couleur ni aucun texte visible codé en dur ailleurs. Les couleurs
//! viennent de [`theme`], les textes de [`t!`].

pub mod hotkey;
pub mod i18n;
pub mod icons;
pub mod keys;
pub mod theme;
pub mod widgets;

pub use i18n::{Lang, set_language};

#[cfg(test)]
mod tests {
    use egui::epaint::text::{Fonts, TextOptions};
    use egui::{FontFamily, FontId};

    #[test]
    fn every_icon_is_in_the_icon_font() {
        let mut fonts = Fonts::new(TextOptions::default(), super::theme::fonts());
        let font = FontId::new(14.0, FontFamily::Proportional);
        for (name, glyph) in super::icons::ALL {
            let c = glyph.chars().next().unwrap();
            assert!(fonts.has_glyph(&font, c), "icône {name} absente");
        }
    }

    /// Aucun caractère des traductions ne doit s'afficher comme un carré.
    #[test]
    fn every_translated_character_has_a_glyph() {
        let mut fonts = Fonts::new(TextOptions::default(), super::theme::fonts());
        let font = FontId::new(14.0, FontFamily::Proportional);
        for source in [
            include_str!("../locales/fr.toml"),
            include_str!("../locales/en.toml"),
        ] {
            for c in source.chars().filter(|c| !c.is_ascii()) {
                assert!(fonts.has_glyph(&font, c), "{c:?} absent des polices");
            }
        }
    }
}
