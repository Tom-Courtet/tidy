//! Enroulement des boîtes (logique pure) : cadre enroulé, bord de l'écran, enroulement
//! automatique (« Smarter Roll »).

use std::time::Duration;

use tidy_core::geometry::Rect;

/// Distance au bord de l'écran sous laquelle une boîte y est « collée ».
const EDGE_TOLERANCE: f32 = 4.0;
/// Délai avant de s'enrouler quand la souris s'éloigne.
pub const AUTO_ROLL_DELAY: Duration = Duration::from_millis(700);

/// Bords de l'écran touchés par une boîte.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Edges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

impl Edges {
    pub fn any(self) -> bool {
        self.left || self.right || self.top || self.bottom
    }
}

/// Bords du cadre utile `bounds` que touche `rect`.
pub fn edges(rect: Rect, bounds: Rect) -> Edges {
    Edges {
        left: (rect.x - bounds.x).abs() <= EDGE_TOLERANCE,
        right: (bounds.right() - rect.right()).abs() <= EDGE_TOLERANCE,
        top: (rect.y - bounds.y).abs() <= EDGE_TOLERANCE,
        bottom: (bounds.bottom() - rect.bottom()).abs() <= EDGE_TOLERANCE,
    }
}

/// Cadre enroulé d'une boîte de cadre `full` : la barre de titre seule. Une boîte
/// collée en bas de l'écran (et pas en haut) s'enroule vers le bas : sa barre reste
/// contre le bord. Rend le cadre et `true` si l'enroulement est ancré en bas.
pub fn rolled(full: Rect, title: f32, bounds: Rect) -> (Rect, bool) {
    let e = edges(full, bounds);
    let bottom = e.bottom && !e.top;
    let y = if bottom {
        full.bottom() - title
    } else {
        full.y
    };
    (Rect::new(full.x, y, full.width, title), bottom)
}

/// Cadre déroulé d'après le cadre enroulé `rolled` (déplacé entre-temps) et la hauteur
/// complète.
pub fn unrolled(rolled: Rect, full_height: f32, anchored_bottom: bool) -> Rect {
    let y = if anchored_bottom {
        rolled.bottom() - full_height
    } else {
        rolled.y
    };
    Rect::new(rolled.x, y, rolled.width, full_height)
}

/// La boîte doit-elle s'enrouler d'elle-même quand la souris s'éloigne ?
/// « Smarter Roll » choisi pour la boîte, ou réglage « enrouler au bord de l'écran ».
pub fn auto_roll(box_auto: bool, at_edge_setting: bool, full: Rect, bounds: Rect) -> bool {
    box_auto || (at_edge_setting && edges(full, bounds).any())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect::new(0.0, 25.0, 1000.0, 775.0);

    #[test]
    fn detects_edges() {
        assert!(!edges(Rect::new(100.0, 100.0, 300.0, 200.0), SCREEN).any());
        let e = edges(Rect::new(0.0, 600.0, 300.0, 200.0), SCREEN);
        assert!(e.left && e.bottom && !e.top && !e.right);
    }

    #[test]
    fn rolls_towards_the_bottom_edge() {
        let full = Rect::new(100.0, 600.0, 300.0, 200.0);
        let (r, bottom) = rolled(full, 30.0, SCREEN);
        assert!(bottom);
        assert_eq!(r, Rect::new(100.0, 770.0, 300.0, 30.0));
        assert_eq!(unrolled(r, 200.0, bottom), full);
        // Ailleurs : vers le haut.
        let free = Rect::new(100.0, 100.0, 300.0, 200.0);
        let (r, bottom) = rolled(free, 30.0, SCREEN);
        assert!(!bottom);
        assert_eq!(r, Rect::new(100.0, 100.0, 300.0, 30.0));
        // Déplacée enroulée, puis déroulée à sa nouvelle place.
        let moved = Rect::new(200.0, 300.0, 300.0, 30.0);
        assert_eq!(
            unrolled(moved, 200.0, false),
            Rect::new(200.0, 300.0, 300.0, 200.0)
        );
    }

    #[test]
    fn auto_roll_by_choice_or_at_the_edge() {
        let free = Rect::new(100.0, 100.0, 300.0, 200.0);
        let edge = Rect::new(700.0, 100.0, 300.0, 200.0);
        assert!(auto_roll(true, false, free, SCREEN));
        assert!(!auto_roll(false, true, free, SCREEN));
        assert!(auto_roll(false, true, edge, SCREEN));
        assert!(!auto_roll(false, false, edge, SCREEN));
    }
}
