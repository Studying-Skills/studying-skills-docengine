//! The display list: what a laid-out document looks like, page by page.

use serde::{Deserialize, Serialize};
use sfdoc_geom::{Point, Rect, Size};
use sfdoc_model::Color;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub pages: Vec<Page>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub size: Size,
    /// In paint order: later items are drawn over earlier ones.
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Item {
    /// A filled rectangle: highlight, table shading, rule, underline.
    Rect {
        rect: Rect,
        color: Color,
    },
    Glyphs(GlyphRun),
}

/// Glyphs of one font, size and colour on one baseline: a word or a part of one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlyphRun {
    /// Index of the font in the engine's font set.
    pub font: u32,
    /// Font size in points.
    pub size: f32,
    /// Start of the run on its baseline, in page coordinates.
    pub origin: Point,
    pub color: Color,
    pub glyphs: Vec<PlacedGlyph>,
    /// The characters the glyphs stand for: painters that draw text (the browser, PDF text) use it.
    pub text: String,
    /// Width of the run in points.
    pub width: f32,
    /// Document positions covered: `from..to`.
    pub from: u32,
    pub to: u32,
}

/// A glyph and its offset from the run's origin in points (`y` grows downwards).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedGlyph {
    pub id: u16,
    pub x: f32,
    pub y: f32,
}
