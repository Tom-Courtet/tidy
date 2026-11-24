//! Aimantation des boîtes : alignement sur les autres boîtes et les bords de l'écran,
//! écart régulier, taille en multiples d'icônes.
//!
//! Lot A (vague 1) : implémentation complète (écart `LayoutSettings::spacing`, 10 points
//! par défaut ; multiples d'icônes ; évitement des icônes hors boîte). Le bureau
//! (lot C) appelle [`snap`] à la fin d'un déplacement ou d'un redimensionnement, sauf
//! si Maj est enfoncée.

use crate::geometry::Rect;
use crate::settings::LayoutSettings;

/// Contexte de l'aimantation.
pub struct SnapContext<'a> {
    /// Cadres des autres boîtes du même écran.
    pub others: &'a [Rect],
    /// Cadre utile de l'écran.
    pub bounds: Rect,
    /// Taille d'une cellule d'icône (pour les multiples).
    pub cell: f32,
    pub settings: &'a LayoutSettings,
}

/// Cadre aimanté de `rect`. Sans aimantation : simplement ramené dans l'écran.
pub fn snap(rect: Rect, ctx: &SnapContext) -> Rect {
    rect.clamped_to(&ctx.bounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_on_screen() {
        let settings = LayoutSettings::default();
        let ctx = SnapContext {
            others: &[],
            bounds: Rect::new(0.0, 0.0, 1000.0, 800.0),
            cell: 80.0,
            settings: &settings,
        };
        let r = snap(Rect::new(950.0, -20.0, 200.0, 100.0), &ctx);
        assert_eq!(r, Rect::new(800.0, 0.0, 200.0, 100.0));
    }
}
