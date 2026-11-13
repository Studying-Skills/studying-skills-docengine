//! The engine as the editor drives it: fonts in, document in, edits in, pages out. No JavaScript types here, so
//! the whole flow is tested natively.

use sfdoc_fonts::{Font, FontSet};
use sfdoc_layout::Layout;
use sfdoc_model::{Block, Document};

#[derive(Default)]
pub struct Engine {
    fonts: FontSet,
    doc: Document,
    /// The layout of `doc`; dropped by every change and rebuilt on demand.
    layout: Option<Layout>,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a font file under the family and style it states; returns `family|bold|italic`.
    pub fn add_font(&mut self, data: Vec<u8>) -> Result<String, String> {
        let font = Font::from_bytes(data).map_err(|e| e.to_string())?;
        let (_, d) = self.fonts.add_described(font).ok_or("the font has no family name")?;
        self.layout = None;
        Ok(format!("{}|{}|{}", d.family, d.bold, d.italic))
    }

    /// Replace the whole document; returns its content size.
    pub fn set_document(&mut self, json: &str) -> Result<u32, String> {
        self.doc = Document::from_json(json).map_err(|e| e.to_string())?;
        self.layout = None;
        Ok(self.doc.content_size())
    }

    /// Replace the top-level blocks `from..to` with the blocks in `json` (an array); returns the new content size.
    /// On an error the document is unchanged.
    pub fn replace_blocks(&mut self, from: usize, to: usize, json: &str) -> Result<u32, String> {
        let blocks: Vec<Block> = serde_json::from_str(json).map_err(|e| format!("malformed blocks JSON: {e}"))?;
        self.doc.replace_blocks(from, to, blocks).map_err(|e| e.to_string())?;
        self.layout = None;
        Ok(self.doc.content_size())
    }

    pub fn content_size(&self) -> u32 {
        self.doc.content_size()
    }

    pub fn block_count(&self) -> usize {
        self.doc.blocks.len()
    }

    pub fn text_between(&self, from: u32, to: u32, block_separator: &str) -> String {
        self.doc.text_between(from, to, block_separator)
    }

    fn ensure_layout(&mut self) -> Result<&Layout, String> {
        if self.layout.is_none() {
            self.layout = Some(sfdoc_layout::layout(&self.doc, &self.fonts).map_err(|e| e.to_string())?);
        }
        self.layout.as_ref().ok_or_else(|| "layout is missing".to_owned())
    }

    /// Lay the document out if it changed; returns the number of pages.
    pub fn layout(&mut self) -> Result<usize, String> {
        Ok(self.ensure_layout()?.pages.len())
    }

    /// The display list of page `index` (from 0) as JSON.
    pub fn page(&mut self, index: usize) -> Result<String, String> {
        let layout = self.ensure_layout()?;
        let page = layout
            .pages
            .get(index)
            .ok_or_else(|| format!("page {index} of {}", layout.pages.len()))?;
        serde_json::to_string(page).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> Vec<u8> {
        std::fs::read(format!(
            "{}/../../assets/fonts/liberation/LiberationSerif-Regular.ttf",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    const DOC: &str = r#"{"version":1,"blocks":[{"type":"paragraph","inlines":[{"type":"text","text":"Перший"}]},{"type":"paragraph","inlines":[{"type":"text","text":"Другий"}]}]}"#;

    #[test]
    fn the_editor_flow() {
        let mut e = Engine::new();
        assert_eq!(e.layout().unwrap_err(), "no fonts are loaded");
        assert_eq!(e.add_font(font()).unwrap(), "Liberation Serif|false|false");
        assert_eq!(e.set_document(DOC).unwrap(), 16);
        assert_eq!(e.layout().unwrap(), 1);
        assert!(e.page(0).unwrap().starts_with(r#"{"size":{"w":595.27"#));
        assert_eq!(e.page(3).unwrap_err(), "page 3 of 1");

        // The editor reports "block 1 became these two blocks".
        let size = e
            .replace_blocks(
                1,
                2,
                r#"[{"type":"paragraph","inlines":[{"type":"text","text":"Новий"}]},{"type":"paragraph"}]"#,
            )
            .unwrap();
        assert_eq!(size, 8 + 7 + 2);
        assert_eq!(e.block_count(), 3);
        assert_eq!(e.text_between(0, size, "|"), "Перший|Новий|");
        assert_eq!(e.layout().unwrap(), 1);
    }

    #[test]
    fn a_rejected_edit_leaves_the_document_alone() {
        let mut e = Engine::new();
        e.set_document(DOC).unwrap();
        assert!(
            e.replace_blocks(0, 9, "[]")
                .unwrap_err()
                .contains("outside the document")
        );
        assert!(
            e.replace_blocks(0, 1, "{")
                .unwrap_err()
                .contains("malformed blocks JSON")
        );
        assert!(
            e.replace_blocks(0, 1, r#"[{"type":"paragraph","level":9}]"#)
                .unwrap_err()
                .contains("heading level")
        );
        assert!(
            e.set_document(r#"{"version":2,"blocks":[]}"#)
                .unwrap_err()
                .contains("unsupported")
        );
        assert_eq!(e.text_between(0, 16, "|"), "Перший|Другий");
        assert!(e.add_font(vec![1, 2, 3]).is_err());
    }
}
