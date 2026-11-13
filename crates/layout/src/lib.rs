//! Layout: a document and a set of fonts in, pages of positioned glyphs and boxes (the display list) out.
//!
//! The display list is the only thing painters see. The browser painter, the PNG rasteriser and the PDF writer all
//! draw from it, which is what makes the screen and the exported file agree.

pub mod display;
pub mod flow;
pub mod geometry;
pub mod para;
pub mod style;

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use sfdoc_fonts::FontSet;
use sfdoc_geom::{Point, Rect, Size};
use sfdoc_model::{Color, Document, LeafKind, ModelError, PageSetup};

pub use display::{GlyphRun, Item, Object, ObjectKind, Page, PlacedGlyph};
pub use flow::{BlockFlow, Entry, PageLayout, PlacedBox, PlacedLine};
pub use geometry::{Caret, PageRect};
use para::{FragKind, Shapers};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LayoutError {
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error("no fonts are loaded")]
    NoFonts,
}

/// A laid-out document: pages of placed lines and boxes, with the geometry queries an editor needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub page: PageSetup,
    pub pages: Vec<PageLayout>,
    /// Size of the document content the layout was made for.
    pub content_size: u32,
}

/// Keeps the per-block results between layouts, so an edit re-lays only the blocks it touched.
#[derive(Default)]
pub struct Layouter {
    cache: Vec<Option<Arc<BlockFlow>>>,
}

impl Layouter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget everything (fonts or page setup changed).
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Mirror of `Document::replace_blocks`: blocks `from..to` were replaced by `count` new ones.
    pub fn replaced(&mut self, from: usize, to: usize, count: usize) {
        if from <= to && to <= self.cache.len() {
            self.cache.splice(from..to, std::iter::repeat_n(None, count));
        } else {
            self.cache.clear();
        }
    }

    pub fn layout(&mut self, doc: &Document, fonts: &FontSet) -> Result<Layout, LayoutError> {
        doc.validate()?;
        if fonts.is_empty() {
            return Err(LayoutError::NoFonts);
        }
        let sh = Shapers::new(fonts);
        let width = (doc.page.width - doc.page.margin_left - doc.page.margin_right).max(1.0);
        self.cache.resize(doc.blocks.len(), None);
        let mut flows = Vec::with_capacity(doc.blocks.len());
        let (mut pos, mut footnotes) = (0u32, 0u32);
        for (block, slot) in doc.blocks.iter().zip(self.cache.iter_mut()) {
            // A cached result holds while the width is the same and, if the block numbers footnotes, while the
            // number of footnotes before it is the same.
            let valid =
                slot.as_ref().is_some_and(|f| f.width == width && (f.footnotes == 0 || f.footnote_base == footnotes));
            if !valid {
                *slot = Some(Arc::new(flow::flatten(block, width, footnotes, &sh).ok_or(LayoutError::NoFonts)?));
            }
            if let Some(f) = slot {
                footnotes += f.footnotes;
                flows.push((pos, Arc::clone(f)));
            }
            pos += block.size();
        }
        let mut pages = flow::paginate(&doc.page, &flows);
        flow::add_running(&mut pages, &doc.page, &doc.running, &sh).ok_or(LayoutError::NoFonts)?;
        Ok(Layout { page: doc.page, pages, content_size: pos })
    }
}

/// Lay the whole document out from scratch.
pub fn layout(doc: &Document, fonts: &FontSet) -> Result<Layout, LayoutError> {
    Layouter::new().layout(doc, fonts)
}

fn line_items(l: &PlacedLine, fonts: &FontSet, positions: bool, items: &mut Vec<Item>) {
    let line = &l.line;
    let baseline = l.y + line.baseline;
    if let Some(bg) = l.background {
        items.push(Item::Rect { rect: Rect::new(l.x, l.y, l.box_w, line.height), color: bg });
    }
    // Highlights first, so they never cover a neighbour's glyphs.
    for f in &line.frags {
        if let FragKind::Text(t) = &f.kind
            && let Some(bg) = t.background
        {
            items.push(Item::Rect {
                rect: Rect::new(l.x + f.x, l.y, f.advance, line.baseline + line.descent),
                color: bg,
            });
        }
    }
    for f in &line.frags {
        match &f.kind {
            FragKind::Text(t) => {
                let origin = Point { x: l.x + f.x, y: baseline - t.rise };
                if !t.glyphs.is_empty() {
                    items.push(Item::Glyphs(GlyphRun {
                        font: t.font.0,
                        size: t.size,
                        origin,
                        color: t.color,
                        glyphs: t.glyphs.clone(),
                        text: t.text.clone(),
                        width: f.advance,
                        link: t.link.clone(),
                    }));
                }
                if (t.underline || t.strike)
                    && let Some(font) = fonts.get(t.font)
                {
                    let m = font.metrics();
                    let k = t.size / m.units_per_em;
                    // Decorations run under trailing spaces too, so an underlined phrase is one unbroken line.
                    let mut bar = |offset: f32, thickness: f32| {
                        let th = (thickness * k).max(0.5);
                        items.push(Item::Rect {
                            rect: Rect::new(origin.x, origin.y + offset * k - th / 2.0, f.advance, th),
                            color: t.color,
                        });
                    };
                    if t.underline {
                        bar(m.underline_offset, m.underline_thickness);
                    }
                    if t.strike {
                        bar(-m.strikeout_offset, m.strikeout_thickness);
                    }
                }
            }
            FragKind::Atom { kind, width, height, depth, data } => {
                let kind = match kind {
                    sfdoc_model::AtomKind::Math => ObjectKind::Math,
                    sfdoc_model::AtomKind::Image => ObjectKind::Image,
                    _ => ObjectKind::Other,
                };
                let rect = Rect::new(l.x + f.x, baseline + depth - height, *width, *height);
                items.push(Item::Object(Object {
                    rect,
                    kind,
                    data: data.clone(),
                    pos: if positions { l.base + f.from } else { 0 },
                }));
            }
            FragKind::Space => {}
        }
    }
}

impl Layout {
    pub fn page_size(&self) -> Size {
        Size { w: self.page.width, h: self.page.height }
    }

    /// The display list of page `index`.
    pub fn page(&self, index: usize, fonts: &FontSet) -> Option<Page> {
        let p = self.pages.get(index)?;
        let mut items = Vec::new();
        for (rect, color) in &p.decor {
            items.push(Item::Rect { rect: *rect, color: *color });
        }
        for e in &p.entries {
            match e {
                Entry::Line(l) => line_items(l, fonts, true, &mut items),
                Entry::Box(b) => match b.kind {
                    LeafKind::Rule => items.push(Item::Rect { rect: b.rect, color: Color::BLACK }),
                    LeafKind::Image | LeafKind::Math | LeafKind::Other if b.rect.w > 0.0 && b.rect.h > 0.0 => {
                        let kind = match b.kind {
                            LeafKind::Image => ObjectKind::Image,
                            LeafKind::Math => ObjectKind::MathBlock,
                            _ => ObjectKind::Other,
                        };
                        items.push(Item::Object(Object { rect: b.rect, kind, data: b.data.clone(), pos: b.pos }));
                    }
                    _ => {}
                },
            }
        }
        for l in &p.running {
            line_items(l, fonts, false, &mut items);
        }
        Some(Page { size: self.page_size(), items })
    }

    /// A number that changes when the page has to be repainted. Positions are left out on purpose: typing a letter
    /// shifts the positions of everything after it without changing how the later pages look.
    ///
    /// It hashes what is drawn, not which allocation holds it: an undo frees a line and the next layout may put
    /// a different line at the same address.
    pub fn fingerprint(&self, index: usize) -> Option<u64> {
        let p = self.pages.get(index)?;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for (rect, color) in &p.decor {
            for v in [rect.x, rect.y, rect.w, rect.h] {
                v.to_bits().hash(&mut h);
            }
            color.hash(&mut h);
        }
        for e in &p.entries {
            match e {
                Entry::Line(l) => hash_line(l, &mut h),
                Entry::Box(b) => {
                    for v in [b.rect.x, b.rect.y, b.rect.w, b.rect.h] {
                        v.to_bits().hash(&mut h);
                    }
                    b.data.hash(&mut h);
                    (b.kind as u8).hash(&mut h);
                }
            }
        }
        0xffu8.hash(&mut h);
        for l in &p.running {
            hash_line(l, &mut h);
        }
        Some(h.finish())
    }
}

/// Everything about a placed line that reaches the screen.
fn hash_line(l: &PlacedLine, h: &mut impl Hasher) {
    let line = &l.line;
    for v in [l.x, l.y, l.box_w, line.height, line.baseline, line.descent] {
        v.to_bits().hash(h);
    }
    l.background.hash(h);
    line.frags.len().hash(h);
    for f in &line.frags {
        f.x.to_bits().hash(h);
        f.advance.to_bits().hash(h);
        match &f.kind {
            FragKind::Text(t) => {
                0u8.hash(h);
                t.text.hash(h);
                t.font.0.hash(h);
                t.size.to_bits().hash(h);
                t.rise.to_bits().hash(h);
                t.color.hash(h);
                t.background.hash(h);
                (t.underline, t.strike).hash(h);
                t.link.hash(h);
            }
            FragKind::Atom { kind, width, height, depth, data } => {
                1u8.hash(h);
                (*kind as u8).hash(h);
                for v in [width, height, depth] {
                    v.to_bits().hash(h);
                }
                data.hash(h);
            }
            FragKind::Space => 2u8.hash(h),
        }
    }
}

#[cfg(test)]
mod tests;
