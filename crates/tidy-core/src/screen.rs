//! Écrans et configurations d'écrans.
//!
//! Comme l'original, Tidy mémorise la position de chaque boîte **par configuration
//! d'écrans** (l'ensemble des écrans branchés) **et par écran**. Brancher un écran
//! externe retrouve la disposition qu'on avait avec lui ; le débrancher retrouve celle
//! de l'écran seul.

use serde::{Deserialize, Serialize};

use crate::geometry::Rect;

/// Identifiant stable d'un écran (fabricant, modèle, numéro de série), fourni par
/// `tidy-platform`. Il survit aux redémarrages et aux changements de port.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ScreenId(pub String);

/// Un écran tel que le voit l'OS au moment présent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Screen {
    pub id: ScreenId,
    /// Nom lisible (« Écran intégré », « DELL U2720Q »…).
    pub name: String,
    /// Cadre complet, en points, coordonnées globales (origine en haut à gauche de
    /// l'écran principal).
    pub frame: Rect,
    /// Cadre utile, sans la barre des menus ni le Dock.
    pub visible_frame: Rect,
    /// Pixels par point (2 sur un écran Retina).
    pub scale: f32,
    /// Écran principal (celui de la barre des menus).
    pub primary: bool,
}

/// Configuration d'écrans : la liste triée des identifiants branchés.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ScreenSetKey(pub String);

impl ScreenSetKey {
    pub fn of(screens: &[Screen]) -> Self {
        let mut ids: Vec<&str> = screens.iter().map(|s| s.id.0.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        Self(ids.join("+"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(id: &str) -> Screen {
        Screen {
            id: ScreenId(id.into()),
            name: id.into(),
            frame: Rect::default(),
            visible_frame: Rect::default(),
            scale: 2.0,
            primary: false,
        }
    }

    #[test]
    fn screen_set_key_ignores_order() {
        let a = ScreenSetKey::of(&[screen("b"), screen("a")]);
        let b = ScreenSetKey::of(&[screen("a"), screen("b")]);
        assert_eq!(a, b);
        assert_eq!(a.0, "a+b");
    }
}
