//! Aimantation des boîtes : alignement sur les autres boîtes et les bords de l'écran,
//! écart régulier, taille en multiples d'icônes.
//!
//! Le bureau (lot C) appelle [`snap`] à la fin d'un déplacement et [`snap_resize`] à la
//! fin d'un redimensionnement, sauf si Maj est enfoncée. Règles :
//! - un bord à moins de [`SNAP_DISTANCE`] points d'un bord de l'écran s'y colle ;
//! - un bord proche du même bord d'une autre boîte s'aligne dessus (gauche sur gauche,
//!   haut sur haut…) ;
//! - une boîte voisine (qui se chevauche sur l'autre axe) se place à
//!   [`LayoutSettings::spacing`] points (10 par défaut) ;
//! - avec [`LayoutSettings::size_in_icons`], la taille devient un multiple entier de
//!   la cellule d'icône.
//!
//! L'évitement des icônes hors boîte reste au bureau : il connaît leur position.

use crate::geometry::Rect;
use crate::settings::LayoutSettings;

/// Distance (en points) sous laquelle un bord est attiré.
pub const SNAP_DISTANCE: f32 = 12.0;

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

/// Bords tirés pendant un redimensionnement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Edges {
    pub left: bool,
    pub top: bool,
    pub right: bool,
    pub bottom: bool,
}

impl Edges {
    /// Le coin bas droit (poignée habituelle).
    pub const BOTTOM_RIGHT: Edges = Edges {
        left: false,
        top: false,
        right: true,
        bottom: true,
    };
}

/// Cadre aimanté de `rect` après un déplacement : la taille ne change pas (sauf
/// multiples d'icônes), la position s'aligne. Le résultat reste dans l'écran.
pub fn snap(rect: Rect, ctx: &SnapContext) -> Rect {
    let mut r = rect;
    if ctx.settings.size_in_icons {
        r.width = icon_multiple(r.width, ctx.cell);
        r.height = icon_multiple(r.height, ctx.cell);
    }
    if ctx.settings.snap {
        let guides = Guides::new(&r, ctx);
        let dx = best_shift(&[(r.x, &guides.left), (r.right(), &guides.right)]);
        let dy = best_shift(&[(r.y, &guides.top), (r.bottom(), &guides.bottom)]);
        r.x += dx.unwrap_or(0.0);
        r.y += dy.unwrap_or(0.0);
    }
    r.clamped_to(&ctx.bounds)
}

/// Cadre aimanté de `rect` après un redimensionnement par les bords `edges` : seuls ces
/// bords bougent. Le résultat reste dans l'écran.
pub fn snap_resize(rect: Rect, edges: Edges, ctx: &SnapContext) -> Rect {
    let (mut left, mut top, mut right, mut bottom) = (rect.x, rect.y, rect.right(), rect.bottom());
    if ctx.settings.snap {
        let guides = Guides::new(&rect, ctx);
        let pull =
            |edge: f32, targets: &[f32]| edge + best_shift(&[(edge, targets)]).unwrap_or(0.0);
        if edges.left {
            left = pull(left, &guides.left);
        }
        if edges.right {
            right = pull(right, &guides.right);
        }
        if edges.top {
            top = pull(top, &guides.top);
        }
        if edges.bottom {
            bottom = pull(bottom, &guides.bottom);
        }
    }
    // Bords ramenés dans l'écran.
    let b = &ctx.bounds;
    left = left.clamp(b.x, b.right());
    right = right.clamp(b.x, b.right());
    top = top.clamp(b.y, b.bottom());
    bottom = bottom.clamp(b.y, b.bottom());
    if ctx.settings.size_in_icons {
        let width = icon_multiple(right - left, ctx.cell);
        let height = icon_multiple(bottom - top, ctx.cell);
        if edges.left && !edges.right {
            left = right - width;
        } else {
            right = left + width;
        }
        if edges.top && !edges.bottom {
            top = bottom - height;
        } else {
            bottom = top + height;
        }
    }
    Rect::new(left, top, (right - left).max(0.0), (bottom - top).max(0.0)).clamped_to(b)
}

/// Plus proche multiple (au moins 1) de `cell`.
fn icon_multiple(size: f32, cell: f32) -> f32 {
    if cell <= 0.0 {
        return size;
    }
    (size / cell).round().max(1.0) * cell
}

/// Positions où chaque bord de la boîte peut se coller.
struct Guides {
    left: Vec<f32>,
    right: Vec<f32>,
    top: Vec<f32>,
    bottom: Vec<f32>,
}

impl Guides {
    fn new(r: &Rect, ctx: &SnapContext) -> Self {
        let gap = ctx.settings.spacing.max(0.0);
        let b = &ctx.bounds;
        let mut g = Guides {
            left: vec![b.x],
            right: vec![b.right()],
            top: vec![b.y],
            bottom: vec![b.bottom()],
        };
        for o in ctx.others {
            // Alignement des bords de même nature.
            g.left.push(o.x);
            g.right.push(o.right());
            g.top.push(o.y);
            g.bottom.push(o.bottom());
            // Voisines côte à côte (elles se chevauchent verticalement) : écart régulier.
            if overlaps(r.y, r.bottom(), o.y, o.bottom()) {
                g.left.push(o.right() + gap);
                g.right.push(o.x - gap);
            }
            // Voisines l'une au-dessus de l'autre.
            if overlaps(r.x, r.right(), o.x, o.right()) {
                g.top.push(o.bottom() + gap);
                g.bottom.push(o.y - gap);
            }
        }
        g
    }
}

/// Les intervalles se chevauchent-ils (à la distance d'aimantation près) ?
fn overlaps(a0: f32, a1: f32, b0: f32, b1: f32) -> bool {
    a0 - SNAP_DISTANCE < b1 && b0 < a1 + SNAP_DISTANCE
}

/// Plus petit déplacement qui colle un des bords à un de ses guides, s'il est sous la
/// distance d'aimantation.
fn best_shift(edges: &[(f32, &[f32])]) -> Option<f32> {
    edges
        .iter()
        .flat_map(|(edge, targets)| targets.iter().map(move |t| t - edge))
        .filter(|d| d.abs() <= SNAP_DISTANCE)
        .min_by(|a, b| a.abs().total_cmp(&b.abs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect::new(0.0, 0.0, 1000.0, 800.0);

    fn ctx<'a>(others: &'a [Rect], settings: &'a LayoutSettings) -> SnapContext<'a> {
        SnapContext {
            others,
            bounds: SCREEN,
            cell: 80.0,
            settings,
        }
    }

    #[test]
    fn stays_on_screen() {
        let settings = LayoutSettings::default();
        let r = snap(Rect::new(950.0, -20.0, 200.0, 100.0), &ctx(&[], &settings));
        assert_eq!(r, Rect::new(800.0, 0.0, 200.0, 100.0));
    }

    #[test]
    fn sticks_to_screen_edges() {
        let settings = LayoutSettings::default();
        let c = ctx(&[], &settings);
        assert_eq!(
            snap(Rect::new(8.0, 300.0, 200.0, 100.0), &c),
            Rect::new(0.0, 300.0, 200.0, 100.0)
        );
        // Bord droit à 11 points du bord de l'écran.
        assert_eq!(
            snap(Rect::new(789.0, 693.0, 200.0, 100.0), &c),
            Rect::new(800.0, 700.0, 200.0, 100.0)
        );
        // Trop loin : rien ne bouge.
        let far = Rect::new(300.0, 300.0, 200.0, 100.0);
        assert_eq!(snap(far, &c), far);
    }

    #[test]
    fn keeps_the_spacing_between_neighbours_and_aligns_edges() {
        let settings = LayoutSettings::default();
        let others = [Rect::new(100.0, 100.0, 200.0, 200.0)];
        let c = ctx(&others, &settings);
        // À droite de la voisine, un peu trop près, le haut presque aligné.
        assert_eq!(
            snap(Rect::new(305.0, 107.0, 150.0, 150.0), &c),
            Rect::new(310.0, 100.0, 150.0, 150.0)
        );
        // En dessous, le bord gauche presque aligné.
        assert_eq!(
            snap(Rect::new(95.0, 318.0, 150.0, 150.0), &c),
            Rect::new(100.0, 310.0, 150.0, 150.0)
        );
        // Sortie de l'écran par la gauche : collée au bord.
        assert_eq!(
            snap(Rect::new(-5.0, 400.0, 85.0, 50.0), &c),
            Rect::new(0.0, 400.0, 85.0, 50.0)
        );
        // Bas aligné sur le bas de la voisine, sans chevauchement horizontal.
        assert_eq!(
            snap(Rect::new(600.0, 195.0, 100.0, 100.0), &c),
            Rect::new(600.0, 200.0, 100.0, 100.0)
        );
        // L'écart ne s'applique qu'entre voisines : loin verticalement, pas de collage.
        let r = Rect::new(305.0, 600.0, 100.0, 100.0);
        assert_eq!(snap(r, &c), r);
        // À gauche d'une voisine : son bord droit s'arrête à 10 points d'elle.
        let others = [Rect::new(500.0, 100.0, 200.0, 200.0)];
        let c = ctx(&others, &settings);
        assert_eq!(
            snap(Rect::new(295.0, 150.0, 200.0, 100.0), &c),
            Rect::new(290.0, 150.0, 200.0, 100.0)
        );
    }

    #[test]
    fn custom_spacing_and_disabled_snapping() {
        let mut settings = LayoutSettings {
            spacing: 20.0,
            ..LayoutSettings::default()
        };
        let others = [Rect::new(100.0, 100.0, 200.0, 200.0)];
        assert_eq!(
            snap(
                Rect::new(315.0, 150.0, 100.0, 100.0),
                &ctx(&others, &settings)
            ),
            Rect::new(320.0, 150.0, 100.0, 100.0)
        );
        settings.snap = false;
        let r = Rect::new(315.0, 150.0, 100.0, 100.0);
        assert_eq!(snap(r, &ctx(&others, &settings)), r);
    }

    #[test]
    fn size_in_icon_multiples() {
        let settings = LayoutSettings {
            size_in_icons: true,
            snap: false,
            ..LayoutSettings::default()
        };
        let c = ctx(&[], &settings);
        assert_eq!(
            snap(Rect::new(300.0, 300.0, 170.0, 30.0), &c),
            Rect::new(300.0, 300.0, 160.0, 80.0)
        );
        // Redimensionnement par le bord gauche : le bord droit reste en place.
        let r = snap_resize(
            Rect::new(300.0, 300.0, 250.0, 160.0),
            Edges {
                left: true,
                ..Edges::default()
            },
            &c,
        );
        assert_eq!(r, Rect::new(310.0, 300.0, 240.0, 160.0));
    }

    #[test]
    fn resizing_moves_only_the_dragged_edges() {
        let settings = LayoutSettings::default();
        let others = [Rect::new(500.0, 100.0, 200.0, 200.0)];
        let c = ctx(&others, &settings);
        // Bord droit tiré près de la voisine : il s'arrête à 10 points d'elle ; le bas
        // s'aligne sur le sien.
        let r = snap_resize(
            Rect::new(100.0, 100.0, 385.0, 205.0),
            Edges::BOTTOM_RIGHT,
            &c,
        );
        assert_eq!(r, Rect::new(100.0, 100.0, 390.0, 200.0));
        // Le bord gauche, non tiré, ne bouge pas même près de l'écran.
        let r = snap_resize(Rect::new(5.0, 100.0, 100.0, 100.0), Edges::BOTTOM_RIGHT, &c);
        assert_eq!(r.x, 5.0);
    }
}
