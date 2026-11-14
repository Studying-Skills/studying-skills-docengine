//! The engine's own document format.
//!
//! A document is a tree: blocks (paragraphs, lists, quotes, tables, leaf blocks) holding inline content (text with
//! marks, inline atoms). It is stored as JSON and knows nothing about the editor that produced it.
//!
//! # Positions
//!
//! One integer addresses any place in the document ("flat token" scheme):
//!
//! * entering or leaving a non-leaf node counts 1;
//! * a leaf block or an inline atom counts 1;
//! * text counts its length in UTF-16 code units.
//!
//! Position 0 is the start of the document content, before the first block. A paragraph with the text `ab` occupies
//! positions 0..4: 0 before it, 1 before `a`, 2 between the letters, 3 after `b`, 4 after the paragraph.
//! Editors built on the same scheme (ProseMirror is one) share positions with the engine without translation.

mod color;
mod pos;
mod text;

use serde::{Deserialize, Serialize};
use sfdoc_geom::{A4, mm};

pub use color::Color;
pub use pos::{Event, Loc};
pub use text::{utf16_len, utf16_slice};

/// Version of the JSON format this crate reads and writes.
pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ModelError {
    #[error("unsupported document format version {0} (this build reads {FORMAT_VERSION})")]
    UnsupportedVersion(u32),
    #[error("block range {from}..{to} is outside the document ({len} blocks)")]
    BadRange { from: usize, to: usize, len: usize },
    #[error("invalid document: {0}")]
    Invalid(String),
    #[error("malformed document JSON: {0}")]
    Json(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub version: u32,
    #[serde(default)]
    pub page: PageSetup,
    #[serde(default, skip_serializing_if = "is_default")]
    pub running: Running,
    /// Document-wide text settings.
    #[serde(default, skip_serializing_if = "is_default")]
    pub text: TextSetup,
    /// What the built-in paragraph styles look like in this document, by name: `body`, `h1`…`h6`, `code`, and any
    /// name a paragraph states in its `style`. A style that is absent keeps the engine's defaults.
    #[serde(default, skip_serializing_if = "Styles::is_empty")]
    pub styles: Styles,
    #[serde(default)]
    pub blocks: Vec<Block>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            page: PageSetup::default(),
            running: Running::default(),
            text: TextSetup::default(),
            styles: Styles::new(),
            blocks: Vec::new(),
        }
    }
}

pub type Styles = std::collections::BTreeMap<String, StyleDef>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSetup {
    /// Break long words at the ends of lines (Cyrillic words, by syllable rules).
    #[serde(default, skip_serializing_if = "is_false")]
    pub hyphenate: bool,
}

/// What a named style changes in the engine's defaults for its kind of paragraph. `None` leaves the default.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleDef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// Font size in points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_height: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_first: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_before: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_after: Option<f32>,
}

/// Where the page number goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PageNumber {
    #[default]
    TopRight,
    BottomCenter,
    None,
}

/// Running header and footer: the same on every page, optionally absent on the first (title) page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Running {
    #[serde(default)]
    pub header: String,
    #[serde(default)]
    pub footer: String,
    #[serde(default)]
    pub page_number: PageNumber,
    /// The first page is a title page: no header, footer or number on it (it still counts as page 1).
    #[serde(default = "yes")]
    pub title_page: bool,
    /// Header and footer of even pages, when they differ from the odd ones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header_even: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer_even: Option<String>,
    /// Pale text across the middle of every page, under the content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watermark: Option<String>,
}

impl Default for Running {
    fn default() -> Self {
        Self {
            header: String::new(),
            footer: String::new(),
            page_number: PageNumber::TopRight,
            title_page: true,
            header_even: None,
            footer_even: None,
            watermark: None,
        }
    }
}

fn yes() -> bool {
    true
}

/// Page size and margins in points. The default is A4 with the DSTU 3008 margins (left 30, right 10, top and
/// bottom 20 mm).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSetup {
    pub width: f32,
    pub height: f32,
    pub margin_top: f32,
    pub margin_right: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
}

impl Default for PageSetup {
    fn default() -> Self {
        Self {
            width: A4.w,
            height: A4.h,
            margin_top: mm(20.0),
            margin_right: mm(10.0),
            margin_bottom: mm(20.0),
            margin_left: mm(30.0),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Block {
    Paragraph(Paragraph),
    List(List),
    Quote(Quote),
    Table(Table),
    Leaf(Leaf),
}

/// A text block: body paragraph (`level` 0) or heading (`level` 1–6).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Paragraph {
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub level: u8,
    #[serde(default, skip_serializing_if = "is_default")]
    pub props: ParaProps,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inlines: Vec<Inline>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

/// Paragraph formatting. `None` means "take it from the style" (resolved by the layout engine).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParaProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    /// Line spacing as a multiple of the font's line height (1.5 = "one and a half").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_height: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_left: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_right: Option<f32>,
    /// First-line indent; negative for a hanging indent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_first: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_before: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_after: Option<f32>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub keep_next: bool,
    /// The paragraph is an entry of the named list (`figure`, `table`): a contents block with the same `list`
    /// shows it with its page number. Headings are entries of the table of contents without saying so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_of: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub keep_lines: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub page_break_before: bool,
    /// Preformatted text (code block): no wrapping at spaces is not implied, only the monospace style.
    #[serde(default, skip_serializing_if = "is_false")]
    pub code: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Inline {
    Text(Text),
    Atom(Atom),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Text {
    pub text: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub marks: Marks,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VertAlign {
    #[default]
    Baseline,
    Sub,
    Sup,
}

/// Character formatting. `None` means "take it from the paragraph's style".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Marks {
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub strike: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub code: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub vert: VertAlign,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// Font size in points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// Comment threads this text belongs to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<String>,
}

/// An indivisible inline object. Its box is measured outside the engine (an image's size, a formula's SVG box).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Atom {
    pub kind: AtomKind,
    /// Box width and height in points; 0 for kinds the engine sizes itself (line break, footnote mark).
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub width: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub height: f32,
    /// How far the box extends below the text baseline, in points.
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub depth: f32,
    /// Kind-specific reference: footnote id, image path, formula source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AtomKind {
    Break,
    Math,
    Footnote,
    Image,
    /// Anything the engine does not know: laid out as a box of the given size.
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct List {
    #[serde(default, skip_serializing_if = "is_false")]
    pub ordered: bool,
    /// Marker or numbering style (`disc`, `dash`, `decimal-paren`, …); `None` is the default for the kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub start: u32,
    pub items: Vec<ListItem>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ListItem {
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Table {
    /// Column widths in points; empty = size the columns by content.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub col_widths: Vec<f32>,
    /// Which grid lines are drawn; all of them when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub borders: Option<Borders>,
    pub rows: Vec<Row>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Borders {
    #[default]
    All,
    /// Lines between the rows, above the first and under the last; no vertical lines.
    Horizontal,
    /// The frame around the table only.
    Outer,
    None,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub cells: Vec<Cell>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cell {
    #[serde(default, skip_serializing_if = "is_false")]
    pub header: bool,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub colspan: u32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub rowspan: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Color>,
    /// Where the content stands in a cell taller than it; at the top when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valign: Option<VAlign>,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VAlign {
    #[default]
    Top,
    Middle,
    Bottom,
}

impl Default for Cell {
    fn default() -> Self {
        Self { header: false, colspan: 1, rowspan: 1, background: None, valign: None, blocks: Vec::new() }
    }
}

/// A block without content: occupies one position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Leaf {
    pub kind: LeafKind,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub width: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub height: f32,
    /// Kind-specific reference: image path, formula source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    /// Where an image or a formula stands across the text area; centred when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    /// Text set at the right edge of the text area, level with the middle of the box: an equation number "(3)".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// For a contents block: the list it shows (see `ParaProps::entry_of`); the headings when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<String>,
    /// A picture aligned left or right lets the paragraphs after it flow beside it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub wrap: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LeafKind {
    Image,
    Math,
    Rule,
    PageBreak,
    Toc,
    /// Takes a position but no space on the page (document settings stored as a node).
    Hidden,
    /// Anything the engine does not know: laid out as a box of the given size.
    Other,
}

fn is_false(v: &bool) -> bool {
    !*v
}
fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}
fn is_zero_f32(v: &f32) -> bool {
    *v == 0.0
}
fn one() -> u32 {
    1
}
fn is_one(v: &u32) -> bool {
    *v == 1
}
fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// Largest nesting of containers the engine accepts; deeper documents are rejected instead of risking the stack.
pub const MAX_DEPTH: usize = 32;

impl Document {
    /// Parse and validate a document.
    pub fn from_json(json: &str) -> Result<Document, ModelError> {
        let doc: Document = serde_json::from_str(json).map_err(|e| ModelError::Json(e.to_string()))?;
        doc.validate()?;
        Ok(doc)
    }

    pub fn to_json(&self) -> String {
        // Serialising plain data cannot fail; an empty string would only hide a bug, so fall back to `{}`.
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_owned())
    }

    /// Check everything the rest of the engine relies on. A document that passes never makes layout panic.
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.version != FORMAT_VERSION {
            return Err(ModelError::UnsupportedVersion(self.version));
        }
        let p = &self.page;
        let lens = [p.width, p.height, p.margin_top, p.margin_right, p.margin_bottom, p.margin_left];
        if lens.iter().any(|v| !v.is_finite() || *v < 0.0) || p.width <= 0.0 || p.height <= 0.0 {
            return Err(ModelError::Invalid("page size and margins must be finite and not negative".into()));
        }
        for s in self.styles.values() {
            let lens = [s.indent_first, s.space_before, s.space_after];
            if lens.iter().flatten().any(|v| !v.is_finite())
                || [s.size, s.line_height].iter().flatten().any(|v| !v.is_finite() || *v <= 0.0)
            {
                return Err(ModelError::Invalid("style sizes and spacing must be finite, sizes positive".into()));
            }
        }
        validate_blocks(&self.blocks, 0)
    }

    /// Replace the top-level blocks `from..to` with `blocks` (the editor's way of reporting an edit).
    pub fn replace_blocks(&mut self, from: usize, to: usize, blocks: Vec<Block>) -> Result<(), ModelError> {
        if from > to || to > self.blocks.len() {
            return Err(ModelError::BadRange { from, to, len: self.blocks.len() });
        }
        validate_blocks(&blocks, 0)?;
        self.blocks.splice(from..to, blocks);
        Ok(())
    }
}

fn finite_non_negative(v: f32) -> bool {
    v.is_finite() && v >= 0.0
}

fn validate_blocks(blocks: &[Block], depth: usize) -> Result<(), ModelError> {
    if depth > MAX_DEPTH {
        return Err(ModelError::Invalid(format!("blocks nested deeper than {MAX_DEPTH}")));
    }
    for b in blocks {
        match b {
            Block::Paragraph(p) => {
                if p.level > 6 {
                    return Err(ModelError::Invalid(format!("heading level {} (max 6)", p.level)));
                }
                let pr = &p.props;
                let lens = [pr.indent_left, pr.indent_right, pr.indent_first, pr.space_before, pr.space_after];
                if lens.iter().flatten().any(|v| !v.is_finite()) {
                    return Err(ModelError::Invalid("paragraph indents and spacing must be finite".into()));
                }
                if pr.line_height.is_some_and(|v| !v.is_finite() || v <= 0.0) {
                    return Err(ModelError::Invalid("line height must be positive".into()));
                }
                for i in &p.inlines {
                    match i {
                        Inline::Text(t) => {
                            if t.marks.size.is_some_and(|v| !v.is_finite() || v <= 0.0) {
                                return Err(ModelError::Invalid("font size must be positive".into()));
                            }
                        }
                        Inline::Atom(a) => {
                            if ![a.width, a.height].iter().all(|v| finite_non_negative(*v)) || !a.depth.is_finite() {
                                return Err(ModelError::Invalid("inline box size must be finite".into()));
                            }
                        }
                    }
                }
            }
            Block::List(l) => {
                for item in &l.items {
                    validate_blocks(&item.blocks, depth + 1)?;
                }
            }
            Block::Quote(q) => validate_blocks(&q.blocks, depth + 1)?,
            Block::Table(t) => {
                if t.col_widths.iter().any(|v| !finite_non_negative(*v)) {
                    return Err(ModelError::Invalid("column widths must be finite and not negative".into()));
                }
                for row in &t.rows {
                    for cell in &row.cells {
                        if cell.colspan == 0 || cell.rowspan == 0 {
                            return Err(ModelError::Invalid("cell spans start at 1".into()));
                        }
                        validate_blocks(&cell.blocks, depth + 1)?;
                    }
                }
            }
            Block::Leaf(l) => {
                if !finite_non_negative(l.width) || !finite_non_negative(l.height) {
                    return Err(ModelError::Invalid("block size must be finite and not negative".into()));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
