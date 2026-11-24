//! Thème sombre : noir, violet (#7F3AED) et lime (#C6FF34).
//! Polices : Outfit pour le texte, Unbounded pour le nom de l'app et les titres,
//! Lucide pour les icônes (voir [`crate::icons`]), DejaVu Sans réduite pour les
//! symboles des raccourcis.
//!
//! Palette et échelle reprises de Snappy, Soundy et Tappy (même famille d'apps).

use std::sync::Arc;

use egui::epaint::text::{FontData, FontDefinitions, FontTweak};
use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, Theme, Visuals};

pub const BG: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
pub const SURFACE: Color32 = Color32::from_rgb(0x10, 0x10, 0x12);
pub const SURFACE_HI: Color32 = Color32::from_rgb(0x1b, 0x1b, 0x1f);
pub const BORDER: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x30);
pub const TEXT: Color32 = Color32::from_rgb(0xf4, 0xf4, 0xf5);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8e, 0x8e, 0x96);
/// Violet : couleur secondaire (survol, sélection).
pub const ACCENT: Color32 = Color32::from_rgb(0x7f, 0x3a, 0xed);
/// Lime : couleur principale (outil actif, cadre de sélection, boutons principaux).
pub const ACCENT_2: Color32 = Color32::from_rgb(0xc6, 0xff, 0x34);
/// Texte posé sur un fond lime.
pub const ON_ACCENT_2: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
pub const DANGER: Color32 = Color32::from_rgb(0xff, 0x4d, 0x4d);
/// Orange : avertissements.
pub const WARNING: Color32 = Color32::from_rgb(0xf5, 0x9e, 0x0b);

pub const CARD_RADIUS: u8 = 16;

/// Échelle typographique.
pub const SIZE_XS: f32 = 10.0;
pub const SIZE_S: f32 = 12.0;
pub const SIZE_M: f32 = 14.0;
pub const SIZE_L: f32 = 16.0;
pub const SIZE_ICON: f32 = 20.0;

/// Luminance relative (WCAG) d'une couleur sRGB.
fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// Couleur de texte lisible sur un fond `fill` : noir sur fond clair, blanc sinon.
pub fn on(fill: Color32) -> Color32 {
    if luminance(fill) > 0.179 {
        ON_ACCENT_2
    } else {
        TEXT
    }
}

const OUTFIT: &[u8] = include_bytes!("../../../assets/fonts/Outfit-Regular.ttf");
const OUTFIT_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Outfit-SemiBold.ttf");
const UNBOUNDED_BOLD: &[u8] = include_bytes!("../../../assets/fonts/Unbounded-Bold.ttf");
/// Icônes Lucide, réduites aux glyphes de [`crate::icons`].
const LUCIDE: &[u8] = include_bytes!("../../../assets/fonts/Lucide.ttf");
/// Symboles des raccourcis et des knobs (⌘ ⌥ ⇧ ⌃ ↺ ↻…), extraits de DejaVu Sans.
const SYMBOLS: &[u8] = include_bytes!("../../../assets/fonts/Symbols.ttf");
const SYMBOLS_NAME: &str = "Symbols";
/// Nom de la police d'icônes dans les [`FontDefinitions`].
const LUCIDE_NAME: &str = "Lucide";

/// Famille du texte en gras (Outfit SemiBold).
const STRONG: &str = "strong";
/// Famille du nom de l'app et des titres (Unbounded Bold).
const DISPLAY: &str = "display";
/// Famille des icônes seules (Lucide). Les icônes s'affichent aussi dans les autres
/// familles, où Lucide est en secours.
pub const ICONS: &str = "icons";

/// Texte en demi-gras.
pub fn strong(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(STRONG.into()))
}

/// Police des titres.
pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY.into()))
}

/// Styles de texte maison pour `RichText`.
pub trait Typo {
    /// Demi-gras (Outfit SemiBold), couleur forte.
    fn bold(self) -> Self;
    /// Police des titres (Unbounded Bold).
    fn title(self) -> Self;
}

impl Typo for egui::RichText {
    fn bold(self) -> Self {
        self.strong().family(FontFamily::Name(STRONG.into()))
    }

    fn title(self) -> Self {
        self.strong().family(FontFamily::Name(DISPLAY.into()))
    }
}

/// Taille des icônes par rapport au texte qui les entoure. Les glyphes de Lucide
/// occupent presque tout le cadratin, plus haut que les capitales d'Outfit.
const ICON_SCALE: f32 = 0.9;

/// Police d'icônes, un peu réduite pour s'aligner sur le texte.
fn lucide() -> FontData {
    FontData::from_static(LUCIDE).tweak(FontTweak {
        scale: ICON_SCALE,
        ..Default::default()
    })
}

/// Polices embarquées. Lucide vient juste après la police principale de chaque famille,
/// pour les icônes de l'interface. Les polices par défaut d'egui restent en secours,
/// pour les caractères absents d'Outfit et d'Unbounded.
pub fn fonts() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    for (name, bytes) in [
        ("Outfit", OUTFIT),
        ("Outfit-SemiBold", OUTFIT_SEMIBOLD),
        ("Unbounded-Bold", UNBOUNDED_BOLD),
    ] {
        defs.font_data
            .insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    defs.font_data
        .insert(LUCIDE_NAME.into(), Arc::new(lucide()));
    defs.font_data.insert(
        SYMBOLS_NAME.into(),
        Arc::new(FontData::from_static(SYMBOLS)),
    );
    let fallback = defs.families[&FontFamily::Proportional].clone();
    let family = |first: &[&str]| -> Vec<String> {
        first
            .iter()
            .copied()
            .chain([LUCIDE_NAME, SYMBOLS_NAME])
            .map(str::to_owned)
            .chain(fallback.iter().cloned())
            .collect()
    };
    defs.families
        .insert(FontFamily::Proportional, family(&["Outfit"]));
    defs.families.insert(
        FontFamily::Name(STRONG.into()),
        family(&["Outfit-SemiBold"]),
    );
    defs.families.insert(
        FontFamily::Name(DISPLAY.into()),
        family(&["Unbounded-Bold", "Outfit-SemiBold"]),
    );
    defs.families
        .insert(FontFamily::Name(ICONS.into()), family(&[]));
    defs
}

/// Uniquement la police d'icônes, sans secours : pour vérifier son contenu.
#[cfg(test)]
pub fn icon_font_only() -> FontDefinitions {
    let mut defs = FontDefinitions::empty();
    defs.font_data
        .insert(LUCIDE_NAME.into(), Arc::new(lucide()));
    for family in [
        FontFamily::Proportional,
        FontFamily::Monospace,
        FontFamily::Name(ICONS.into()),
    ] {
        defs.families.insert(family, vec![LUCIDE_NAME.into()]);
    }
    defs
}

/// Applique le thème au contexte (une seule fois, au démarrage).
pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_theme(Theme::Dark);
    ctx.style_mut_of(Theme::Dark, |style| {
        let radius = CornerRadius::same(10);
        let mut v = Visuals::dark();
        v.panel_fill = BG;
        v.window_fill = SURFACE;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = CornerRadius::same(CARD_RADIUS);
        v.menu_corner_radius = CornerRadius::same(CARD_RADIUS);
        v.extreme_bg_color = Color32::from_rgb(0x08, 0x08, 0x0a);
        v.faint_bg_color = SURFACE;
        v.override_text_color = Some(TEXT);
        v.hyperlink_color = ACCENT_2;
        // Remplissage des curseurs et éléments sélectionnés : lime, texte noir.
        v.selection.bg_fill = ACCENT_2;
        v.selection.stroke = Stroke::new(1.0, ON_ACCENT_2);
        v.slider_trailing_fill = true;
        v.handle_shape = egui::style::HandleShape::Circle;

        let w = &mut v.widgets;
        w.noninteractive.bg_fill = SURFACE;
        w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
        for (state, fill) in [
            (&mut w.inactive, SURFACE_HI),
            (&mut w.hovered, Color32::from_rgb(0x26, 0x26, 0x2c)),
            (&mut w.active, Color32::from_rgb(0x30, 0x30, 0x38)),
            (&mut w.open, SURFACE_HI),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.corner_radius = radius;
        }
        w.noninteractive.corner_radius = radius;
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
        w.active.bg_stroke = Stroke::new(1.0, ACCENT_2);

        style.visuals = v;
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.slider_width = 110.0;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_on_fill_is_readable() {
        assert_eq!(on(ACCENT_2), ON_ACCENT_2);
        assert_eq!(on(ACCENT), TEXT);
        assert_eq!(on(BG), TEXT);
        assert_eq!(on(Color32::WHITE), ON_ACCENT_2);
    }

    #[test]
    fn custom_fonts_cover_french_and_shortcut_symbols() {
        use egui::epaint::text::{Fonts, TextOptions};
        let mut fonts = Fonts::new(TextOptions::default(), super::fonts());
        for font in [FontId::proportional(14.0), strong(14.0), display(14.0)] {
            // Accents, et symboles des raccourcis et des knobs.
            for c in "Paramètres éàçùœ «» … ’ ⌘⌥⇧⌃ ↺↻ ←→↑↓ ↩⌫⌦⇥ −".chars()
            {
                assert!(fonts.has_glyph(&font, c), "{c:?} absent de {font:?}");
            }
        }
    }
}
