//! Layout: a document and a set of fonts in, pages of positioned glyphs and boxes (the display list) out.
//!
//! The display list is the only thing painters see. The browser painter, the PNG rasteriser and the PDF writer all
//! draw from it, which is what makes the screen and the exported file agree.

pub mod display;

use sfdoc_fonts::FontSet;
use sfdoc_geom::Size;
use sfdoc_model::{Document, ModelError};

pub use display::{GlyphRun, Item, Layout, Page, PlacedGlyph};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LayoutError {
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error("no fonts are loaded")]
    NoFonts,
}

/// Lay the whole document out.
///
/// Stage E1: page geometry only. A document always has at least one page.
pub fn layout(doc: &Document, fonts: &FontSet) -> Result<Layout, LayoutError> {
    doc.validate()?;
    if fonts.is_empty() {
        return Err(LayoutError::NoFonts);
    }
    Ok(Layout {
        pages: vec![Page {
            size: Size {
                w: doc.page.width,
                h: doc.page.height,
            },
            items: Vec::new(),
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sfdoc_fonts::Font;

    fn fonts() -> FontSet {
        let mut set = FontSet::new();
        let path = format!(
            "{}/../../assets/fonts/liberation/LiberationSerif-Regular.ttf",
            env!("CARGO_MANIFEST_DIR")
        );
        set.add_described(Font::from_bytes(std::fs::read(path).unwrap()).unwrap())
            .unwrap();
        set
    }

    #[test]
    fn an_empty_document_is_one_a4_page() {
        let l = layout(&Document::default(), &fonts()).unwrap();
        assert_eq!(l.pages.len(), 1);
        assert!((l.pages[0].size.w - 595.2756).abs() < 0.001 && (l.pages[0].size.h - 841.8898).abs() < 0.001);
    }

    #[test]
    fn layout_refuses_invalid_input_instead_of_panicking() {
        assert_eq!(layout(&Document::default(), &FontSet::new()), Err(LayoutError::NoFonts));
        let bad = Document {
            version: 7,
            ..Default::default()
        };
        assert_eq!(
            layout(&bad, &fonts()),
            Err(LayoutError::Model(ModelError::UnsupportedVersion(7)))
        );
    }
}
