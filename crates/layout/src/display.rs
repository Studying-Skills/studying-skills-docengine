//! The display list: what a laid-out document looks like, page by page.

use serde::{Deserialize, Serialize};
use sfdoc_geom::{Point, Rect, Size};
use sfdoc_model::Color;

/// One page ready to paint.
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
    /// A box the painter fills itself: a picture or a formula.
    Object(Object),
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
    /// Width of the run in points, trailing spaces included.
    pub width: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

/// A glyph and its offset from the run's origin in points (`y` grows downwards).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedGlyph {
    pub id: u16,
    /// Byte offset in the run's text of the first character the glyph stands for. The browser draws text itself
    /// and does not need it, so it stays out of the JSON.
    #[serde(default, skip_serializing)]
    pub cluster: u32,
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Object {
    pub rect: Rect,
    pub kind: ObjectKind,
    /// Image path or formula source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    /// Document position of the object.
    pub pos: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ObjectKind {
    Image,
    /// A formula inside a line of text.
    Math,
    /// A formula on its own line.
    MathBlock,
    Other,
}
