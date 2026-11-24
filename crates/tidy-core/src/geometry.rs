//! Géométrie en points (pas en pixels), origine en haut à gauche de l'écran principal,
//! y vers le bas : la convention de CoreGraphics et d'egui.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn origin(&self) -> Point {
        Point {
            x: self.x,
            y: self.y,
        }
    }

    pub fn size(&self) -> Size {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// Ramène le rectangle dans `bounds` (en le déplaçant, puis en le réduisant s'il
    /// est plus grand).
    pub fn clamped_to(&self, bounds: &Rect) -> Rect {
        let width = self.width.min(bounds.width);
        let height = self.height.min(bounds.height);
        let x = self.x.clamp(bounds.x, bounds.right() - width);
        let y = self.y.clamp(bounds.y, bounds.bottom() - height);
        Rect::new(x, y, width, height)
    }

    /// Même rectangle, exprimé relativement à l'origine de `frame`.
    pub fn relative_to(&self, frame: &Rect) -> Rect {
        Rect::new(self.x - frame.x, self.y - frame.y, self.width, self.height)
    }

    /// Inverse de [`Rect::relative_to`].
    pub fn absolute_in(&self, frame: &Rect) -> Rect {
        Rect::new(self.x + frame.x, self.y + frame.y, self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_moves_then_shrinks() {
        let screen = Rect::new(0.0, 0.0, 100.0, 100.0);
        let r = Rect::new(90.0, -10.0, 20.0, 20.0).clamped_to(&screen);
        assert_eq!(r, Rect::new(80.0, 0.0, 20.0, 20.0));
        let big = Rect::new(0.0, 0.0, 300.0, 50.0).clamped_to(&screen);
        assert_eq!(big, Rect::new(0.0, 0.0, 100.0, 50.0));
    }

    #[test]
    fn relative_round_trip() {
        let frame = Rect::new(1512.0, -200.0, 1920.0, 1080.0);
        let r = Rect::new(1600.0, 0.0, 300.0, 200.0);
        assert_eq!(r.relative_to(&frame).absolute_in(&frame), r);
    }
}
