//! Units and rectangles. Every length inside the engine is in points (1/72 inch), `f32`.
//!
//! Other units exist only at the edges: millimetres and centimetres in page settings, twips and EMU in DOCX,
//! CSS pixels in the browser painter.

use serde::{Deserialize, Serialize};

pub const PT_PER_INCH: f32 = 72.0;
pub const PT_PER_MM: f32 = PT_PER_INCH / 25.4;
pub const PT_PER_CM: f32 = PT_PER_INCH / 2.54;
/// CSS pixel: 1/96 inch.
pub const PT_PER_PX: f32 = PT_PER_INCH / 96.0;
/// Twip: 1/20 point (DOCX page sizes and indents).
pub const PT_PER_TWIP: f32 = 1.0 / 20.0;
/// English Metric Unit: 1/914400 inch (DOCX drawings).
pub const PT_PER_EMU: f32 = PT_PER_INCH / 914_400.0;

pub fn mm(v: f32) -> f32 {
    v * PT_PER_MM
}
pub fn cm(v: f32) -> f32 {
    v * PT_PER_CM
}
pub fn px(v: f32) -> f32 {
    v * PT_PER_PX
}
pub fn to_px(pt: f32) -> f32 {
    pt / PT_PER_PX
}
pub fn to_mm(pt: f32) -> f32 {
    pt / PT_PER_MM
}
/// Points to twips, rounded to the nearest twip (what DOCX stores).
#[expect(
    clippy::cast_possible_truncation,
    reason = "the float to int cast saturates, and a page length is far below i32::MAX twips"
)]
pub fn to_twips(pt: f32) -> i32 {
    (pt / PT_PER_TWIP).round() as i32
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

/// Axis-aligned rectangle; `y` grows downwards (page coordinates).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    /// True for a point inside or on the left/top edge (half-open, so adjacent rectangles do not both match).
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }
    /// The rectangle shrunk by the given insets; never negative in size.
    pub fn inset(&self, left: f32, top: f32, right: f32, bottom: f32) -> Rect {
        Rect::new(self.x + left, self.y + top, (self.w - left - right).max(0.0), (self.h - top - bottom).max(0.0))
    }
}

/// A4 in points.
pub const A4: Size = Size { w: 210.0 * PT_PER_MM, h: 297.0 * PT_PER_MM };

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_conversions() {
        assert!((mm(25.4) - 72.0).abs() < 1e-4);
        assert!((cm(2.54) - 72.0).abs() < 1e-4);
        assert!((px(96.0) - 72.0).abs() < 1e-4);
        assert!((to_px(14.0) - 18.666_666).abs() < 1e-4);
        assert_eq!(to_twips(A4.w), 11906);
        assert_eq!(to_twips(A4.h), 16838);
        assert!((to_mm(mm(30.0)) - 30.0).abs() < 1e-4);
    }

    #[test]
    fn rect_edges_are_half_open() {
        let r = Rect::new(10.0, 10.0, 5.0, 5.0);
        assert!(r.contains(Point { x: 10.0, y: 10.0 }));
        assert!(!r.contains(Point { x: 15.0, y: 12.0 }));
        assert!(!r.intersects(&Rect::new(15.0, 10.0, 5.0, 5.0)));
        assert!(r.intersects(&Rect::new(14.9, 14.9, 5.0, 5.0)));
    }

    #[test]
    fn inset_never_goes_negative() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0).inset(8.0, 0.0, 8.0, 0.0);
        assert_eq!(r.w, 0.0);
    }
}
