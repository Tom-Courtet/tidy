//! Conversions pures (pixels, rectangles, seuils, chemins), sans appel système :
//! testées sur tous les OS, utilisées par `windows/` et `macos/`.

// Chaque OS n'en utilise qu'une partie.
#![allow(dead_code)]

use tidy_core::geometry::{Point, Rect};

/// Côté maximal d'une icône demandée, en pixels (au-delà, on réduit).
pub const MAX_ICON_PX: u32 = 1024;

/// Côté minimal, en points, d'un rectangle tracé sur le bureau pour qu'il compte comme
/// « dessiner une boîte » (en dessous : simple clic ou petit glissé).
pub const MIN_SELECTION_SIDE: f32 = 32.0;

/// Taille de l'icône réellement demandée au système (1 à [`MAX_ICON_PX`]).
pub fn icon_side(size_px: u32) -> u32 {
    size_px.clamp(1, MAX_ICON_PX)
}

/// Une composante prémultipliée `c` (0 à `a`) ramenée à sa valeur d'origine.
fn unpremultiply(c: u8, a: u8) -> u8 {
    match a {
        0 => 0,
        255 => c,
        a => ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8,
    }
}

/// RGBA prémultiplié → RGBA non prémultiplié, sur place.
pub fn unpremultiply_rgba(rgba: &mut [u8]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = px[3];
        if a == 0 {
            px[..3].fill(0);
        } else {
            for c in &mut px[..3] {
                *c = unpremultiply(*c, a);
            }
        }
    }
}

/// Pixels BGRA 32 bits (DIB de Windows) → RGBA non prémultiplié.
///
/// Trois cas :
/// - aucun pixel n'a d'alpha (tout à 0) : image sans canal alpha (vieilles icônes,
///   miniatures JPEG), rendue opaque ;
/// - une composante dépasse son alpha : l'image n'est pas prémultipliée, on ne fait
///   que réordonner les octets ;
/// - sinon : alpha prémultiplié (cas normal des icônes), on le retire.
pub fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    let pixels = bgra.as_chunks::<4>().0;
    let no_alpha = pixels.iter().all(|px| px[3] == 0);
    let straight = !no_alpha && pixels.iter().any(|px| px[..3].iter().any(|&c| c > px[3]));
    let mut out = Vec::with_capacity(pixels.len() * 4);
    for &[b, g, r, a] in pixels {
        if no_alpha {
            out.extend_from_slice(&[r, g, b, 255]);
        } else if straight {
            out.extend_from_slice(&[r, g, b, a]);
        } else {
            out.extend_from_slice(&[
                unpremultiply(r, a),
                unpremultiply(g, a),
                unpremultiply(b, a),
                a,
            ]);
        }
    }
    out
}

/// Rectangle (en points) tracé de `start` à `end`, s'il est assez grand pour dessiner
/// une boîte : au moins `min_side` points de large **et** de haut.
pub fn selection_rect(start: Point, end: Point, min_side: f32) -> Option<Rect> {
    let (x, y) = (start.x.min(end.x), start.y.min(end.y));
    let (w, h) = ((start.x - end.x).abs(), (start.y - end.y).abs());
    (w >= min_side && h >= min_side).then(|| Rect::new(x, y, w, h))
}

/// Un écran vu en pixels physiques et en points (voir `windows/screens.rs`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicalScreen {
    /// Cadre physique : gauche, haut, droite, bas (droite et bas exclus).
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    /// Origine du cadre logique, en points.
    pub origin: Point,
    /// Pixels physiques par point.
    pub scale: f32,
}

/// Point physique → points, avec l'échelle de l'écran qui le contient. Hors de tout
/// écran : l'écran le plus proche, sinon les pixels tels quels.
pub fn physical_to_points(x: i32, y: i32, screens: &[PhysicalScreen]) -> Point {
    let inside = |s: &&PhysicalScreen| x >= s.left && x < s.right && y >= s.top && y < s.bottom;
    let gap = |s: &&PhysicalScreen| {
        let dx = (s.left - x).max(0).max(x - (s.right - 1)).max(0) as i64;
        let dy = (s.top - y).max(0).max(y - (s.bottom - 1)).max(0) as i64;
        dx * dx + dy * dy
    };
    match screens
        .iter()
        .find(inside)
        .or_else(|| screens.iter().min_by_key(gap))
    {
        Some(s) if s.scale > 0.0 => Point {
            x: s.origin.x + (x - s.left) as f32 / s.scale,
            y: s.origin.y + (y - s.top) as f32 / s.scale,
        },
        _ => Point {
            x: x as f32,
            y: y as f32,
        },
    }
}

/// Chemin tel que le Shell de Windows l'accepte : sans le préfixe `\\?\` des chemins
/// longs (`\\?\UNC\serveur\…` redevient `\\serveur\…`).
pub fn shell_path(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplied_icons_are_restored() {
        // Rouge pur à moitié transparent : prémultiplié (128, 0, 0, 128).
        let bgra = [0, 0, 128, 128, 0, 0, 0, 0, 10, 20, 30, 255];
        assert_eq!(
            bgra_to_rgba(&bgra),
            vec![255, 0, 0, 128, 0, 0, 0, 0, 30, 20, 10, 255]
        );
    }

    #[test]
    fn images_without_alpha_become_opaque() {
        let bgra = [1, 2, 3, 0, 4, 5, 6, 0];
        assert_eq!(bgra_to_rgba(&bgra), vec![3, 2, 1, 255, 6, 5, 4, 255]);
    }

    #[test]
    fn straight_alpha_is_left_alone() {
        // 200 > 100 : impossible en prémultiplié, l'image est en alpha direct.
        let bgra = [0, 0, 200, 100, 50, 50, 50, 255];
        assert_eq!(bgra_to_rgba(&bgra), vec![200, 0, 0, 100, 50, 50, 50, 255]);
    }

    #[test]
    fn incomplete_pixels_are_ignored() {
        assert_eq!(bgra_to_rgba(&[1, 2, 3, 255, 9]), vec![3, 2, 1, 255]);
    }

    #[test]
    fn unpremultiply_in_place() {
        let mut rgba = [64, 32, 0, 128, 7, 7, 7, 0, 255, 255, 255, 255];
        unpremultiply_rgba(&mut rgba);
        assert_eq!(rgba, [128, 64, 0, 128, 0, 0, 0, 0, 255, 255, 255, 255]);
    }

    #[test]
    fn icon_side_is_bounded() {
        assert_eq!(icon_side(0), 1);
        assert_eq!(icon_side(48), 48);
        assert_eq!(icon_side(100_000), MAX_ICON_PX);
    }

    #[test]
    fn selection_needs_both_sides_above_threshold() {
        let p = |x, y| Point { x, y };
        assert_eq!(
            selection_rect(p(100.0, 80.0), p(20.0, 10.0), 32.0),
            Some(Rect::new(20.0, 10.0, 80.0, 70.0))
        );
        assert_eq!(selection_rect(p(0.0, 0.0), p(200.0, 10.0), 32.0), None);
        assert_eq!(selection_rect(p(0.0, 0.0), p(5.0, 5.0), 32.0), None);
        assert!(selection_rect(p(0.0, 0.0), p(32.0, 32.0), 32.0).is_some());
    }

    fn screens() -> Vec<PhysicalScreen> {
        vec![
            // Écran principal 2560×1440 à 200 % : 1280×720 points.
            PhysicalScreen {
                left: 0,
                top: 0,
                right: 2560,
                bottom: 1440,
                origin: Point { x: 0.0, y: 0.0 },
                scale: 2.0,
            },
            // À droite, 1920×1080 à 100 %.
            PhysicalScreen {
                left: 2560,
                top: 0,
                right: 4480,
                bottom: 1080,
                origin: Point { x: 2560.0, y: 0.0 },
                scale: 1.0,
            },
        ]
    }

    #[test]
    fn physical_points_use_their_screen_scale() {
        let s = screens();
        assert_eq!(
            physical_to_points(200, 100, &s),
            Point { x: 100.0, y: 50.0 }
        );
        assert_eq!(
            physical_to_points(2600, 100, &s),
            Point {
                x: 2600.0,
                y: 100.0
            }
        );
        // Sous le second écran (plus bas que lui) : le plus proche, le second.
        assert_eq!(
            physical_to_points(3000, 1200, &s),
            Point {
                x: 3000.0,
                y: 1200.0
            }
        );
        // Aucun écran : pixels tels quels.
        assert_eq!(physical_to_points(7, 9, &[]), Point { x: 7.0, y: 9.0 });
    }

    #[test]
    fn long_path_prefix_is_removed() {
        assert_eq!(
            shell_path(r"\\?\C:\Users\a\Desktop\x.txt"),
            r"C:\Users\a\Desktop\x.txt"
        );
        assert_eq!(shell_path(r"\\?\UNC\srv\share\f"), r"\\srv\share\f");
        assert_eq!(shell_path(r"C:\a"), r"C:\a");
    }
}
