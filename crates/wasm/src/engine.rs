//! The engine as the editor drives it: fonts in, document in, edits in, pages out. No JavaScript types here, so
//! the whole flow is tested natively.

use sfdoc_fonts::{Font, FontSet};
use sfdoc_layout::{Layout, Layouter};
use sfdoc_model::{Block, Document};

#[derive(Default)]
pub struct Engine {
    fonts: FontSet,
    doc: Document,
    layouter: Layouter,
    /// The layout of `doc`; dropped by every change and rebuilt on demand.
    layout: Option<Layout>,
    /// Pictures and formula drawings for the PDF, by the `data` of their objects.
    pictures: std::collections::HashMap<String, sfdoc_pdf::Picture>,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a font file under the family and style it states; returns `family|bold|italic`.
    pub fn add_font(&mut self, data: Vec<u8>) -> Result<String, String> {
        let font = Font::from_bytes(data).map_err(|e| e.to_string())?;
        let (_, d) = self.fonts.add_described(font).ok_or("the font has no family name")?;
        self.layouter.clear();
        self.layout = None;
        Ok(format!("{}|{}|{}", d.family, d.bold, d.italic))
    }

    /// Replace the whole document; returns its content size. Page settings set earlier are kept unless the
    /// document states its own.
    pub fn set_document(&mut self, json: &str) -> Result<u32, String> {
        self.doc = Document::from_json(json).map_err(|e| e.to_string())?;
        self.layouter.clear();
        self.layout = None;
        Ok(self.doc.content_size())
    }

    /// Replace the top-level blocks `from..to` with the blocks in `json` (an array); returns the new content size.
    /// On an error the document is unchanged.
    pub fn replace_blocks(&mut self, from: usize, to: usize, json: &str) -> Result<u32, String> {
        let blocks: Vec<Block> = serde_json::from_str(json).map_err(|e| format!("malformed blocks JSON: {e}"))?;
        let count = blocks.len();
        self.doc.replace_blocks(from, to, blocks).map_err(|e| e.to_string())?;
        self.layouter.replaced(from, to, count);
        self.layout = None;
        Ok(self.doc.content_size())
    }

    /// Page size, margins and running header/footer as JSON: `{"page": {...}, "running": {...}}`; either part
    /// may be left out. Refused settings change nothing.
    pub fn set_page(&mut self, json: &str) -> Result<(), String> {
        #[derive(serde::Deserialize)]
        struct Settings {
            page: Option<sfdoc_model::PageSetup>,
            running: Option<sfdoc_model::Running>,
        }
        let s: Settings = serde_json::from_str(json).map_err(|e| format!("malformed page settings: {e}"))?;
        let page = s.page.unwrap_or(self.doc.page);
        let running = s.running.unwrap_or_else(|| self.doc.running.clone());
        Document { version: self.doc.version, page, running: running.clone(), blocks: Vec::new() }
            .validate()
            .map_err(|e| e.to_string())?;
        if page != self.doc.page || running != self.doc.running {
            self.doc.page = page;
            self.doc.running = running;
            self.layout = None;
        }
        Ok(())
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
            self.layout = Some(self.layouter.layout(&self.doc, &self.fonts).map_err(|e| e.to_string())?);
        }
        self.layout.as_ref().ok_or_else(|| "layout is missing".to_owned())
    }

    /// Lay the document out if it changed; returns one fingerprint per page (a page whose fingerprint did not
    /// change looks the same and need not be repainted).
    pub fn layout(&mut self) -> Result<Vec<f64>, String> {
        let l = self.ensure_layout()?;
        // JavaScript numbers hold 53 bits exactly: fold the 64-bit fingerprint down.
        Ok((0..l.pages.len()).filter_map(|i| l.fingerprint(i)).map(|h| ((h >> 11) ^ (h & 0x7ff)) as f64).collect())
    }

    /// Page width and height in points.
    pub fn page_size(&self) -> Vec<f32> {
        vec![self.doc.page.width, self.doc.page.height]
    }

    /// The display list of page `index` (from 0) as JSON.
    pub fn page(&mut self, index: usize) -> Result<String, String> {
        self.ensure_layout()?;
        let Some(layout) = self.layout.as_ref() else { return Err("layout is missing".into()) };
        let page = layout.page(index, &self.fonts).ok_or_else(|| format!("page {index} of {}", layout.pages.len()))?;
        serde_json::to_string(&page).map_err(|e| e.to_string())
    }

    /// Caret for a position: `[page, x, y, height]`, empty when the document has no place for a caret.
    pub fn caret_rect(&mut self, pos: u32) -> Result<Vec<f32>, String> {
        Ok(self.ensure_layout()?.caret_rect(pos).map_or_else(Vec::new, |c| vec![c.page as f32, c.x, c.y, c.height]))
    }

    /// Position nearest to a point of a page; -1 when the document is empty.
    pub fn hit_test(&mut self, page: usize, x: f32, y: f32) -> Result<i32, String> {
        Ok(self.ensure_layout()?.hit_test(page, x, y).map_or(-1, |p| p as i32))
    }

    /// Selection rectangles as a flat list: `page, x, y, width, height` for every rectangle.
    pub fn selection_rects(&mut self, from: u32, to: u32) -> Result<Vec<f32>, String> {
        let rects = self.ensure_layout()?.selection_rects(from, to);
        Ok(rects.iter().flat_map(|r| [r.page as f32, r.rect.x, r.rect.y, r.rect.w, r.rect.h]).collect())
    }

    /// Position one line up or down from `pos`, keeping page x `goal_x`.
    pub fn line_move(&mut self, pos: u32, down: bool, goal_x: f32) -> Result<u32, String> {
        Ok(self.ensure_layout()?.line_move(pos, down, goal_x).unwrap_or(pos))
    }

    /// First and last position of the line `pos` is on: `[start, end]`.
    pub fn line_bounds(&mut self, pos: u32) -> Result<Vec<u32>, String> {
        Ok(self.ensure_layout()?.line_bounds(pos).map_or_else(|| vec![pos, pos], |(a, b)| vec![a, b]))
    }

    /// Give the PDF writer a picture file for objects whose `data` is `key`.
    pub fn add_picture(&mut self, key: &str, bytes: Vec<u8>) {
        self.pictures.insert(key.to_owned(), sfdoc_pdf::Picture::Raster(bytes));
    }

    /// Give the PDF writer an SVG (a formula drawn as outlines, or an SVG picture) for objects whose `data` is `key`.
    pub fn add_svg(&mut self, key: &str, svg: &str) {
        self.pictures.insert(key.to_owned(), sfdoc_pdf::Picture::Svg(svg.to_owned()));
    }

    pub fn clear_pictures(&mut self) {
        self.pictures.clear();
    }

    /// The document as PDF, and the `data` of every object that had no usable picture.
    pub fn pdf(&mut self, title: &str) -> Result<(Vec<u8>, Vec<String>), String> {
        self.ensure_layout()?;
        let Some(layout) = self.layout.as_ref() else { return Err("layout is missing".into()) };
        let info = sfdoc_pdf::Info {
            title: (!title.is_empty()).then(|| title.to_owned()),
            language: Some("uk".into()),
            ..Default::default()
        };
        let (bytes, report) =
            sfdoc_pdf::write_pdf(layout, &self.fonts, &self.pictures, &info).map_err(|e| e.to_string())?;
        Ok((bytes, report.missing))
    }

    /// Page (from 0) a position is on; -1 when the document is empty.
    pub fn page_of(&mut self, pos: u32) -> Result<i32, String> {
        Ok(self.ensure_layout()?.page_of(pos).map_or(-1, |p| p as i32))
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
        let before = e.layout().unwrap();
        assert_eq!(before.len(), 1);
        assert!(e.page(0).unwrap().contains(r#""text":"Перший""#));
        assert_eq!(e.page(3).unwrap_err(), "page 3 of 1");
        assert!((e.page_size()[0] - 595.2756).abs() < 0.001 && (e.page_size()[1] - 841.8898).abs() < 0.001);

        // The editor reports "block 1 became these two blocks".
        let json = r#"[{"type":"paragraph","inlines":[{"type":"text","text":"Новий"}]},{"type":"paragraph"}]"#;
        let size = e.replace_blocks(1, 2, json).unwrap();
        assert_eq!(size, 8 + 7 + 2);
        assert_eq!(e.block_count(), 3);
        assert_eq!(e.text_between(0, size, "|"), "Перший|Новий|");
        let after = e.layout().unwrap();
        assert_ne!(before, after);
        // The same layout asked for again: the same fingerprints.
        assert_eq!(e.layout().unwrap(), after);
        assert!(after.iter().all(|f| *f >= 0.0 && *f < 9_007_199_254_740_992.0 && f.fract() == 0.0));
    }

    #[test]
    fn geometry_queries_answer_in_flat_numbers() {
        let mut e = Engine::new();
        e.add_font(font()).unwrap();
        // Nothing to put a caret on yet.
        assert!(e.caret_rect(0).unwrap().is_empty());
        assert_eq!(e.hit_test(0, 10.0, 10.0).unwrap(), -1);
        assert_eq!(e.page_of(0).unwrap(), -1);
        e.set_document(DOC).unwrap();
        let c = e.caret_rect(3).unwrap();
        assert_eq!(c.len(), 4);
        assert_eq!(e.hit_test(0, c[1], c[2] + 1.0).unwrap(), 3);
        assert_eq!(e.selection_rects(1, 7).unwrap().len(), 5);
        assert_eq!(e.selection_rects(1, 12).unwrap().len(), 10);
        assert_eq!(e.line_move(3, true, c[1]).unwrap(), 11);
        assert_eq!(e.line_bounds(11).unwrap(), vec![9, 15]);
        assert_eq!(e.page_of(11).unwrap(), 0);
    }

    #[test]
    fn page_settings_change_the_layout_and_bad_ones_are_refused() {
        let mut e = Engine::new();
        e.add_font(font()).unwrap();
        e.set_document(DOC).unwrap();
        let x = e.caret_rect(1).unwrap()[1];
        let narrow = r#"{"page":{"width":595.28,"height":841.89,"marginTop":50,"marginRight":50,"marginBottom":50,"marginLeft":50}}"#;
        e.set_page(narrow).unwrap();
        assert!((x - e.caret_rect(1).unwrap()[1] - (85.039 - 50.0)).abs() < 0.01);
        e.set_page(r#"{"running":{"header":"Шапка","footer":"","pageNumber":"bottomCenter","titlePage":false}}"#)
            .unwrap();
        let page = e.page(0).unwrap();
        assert!(page.contains(r#""text":"Шапка""#) && page.contains(r#""text":"1""#));
        let bad =
            r#"{"page":{"width":-1,"height":841.89,"marginTop":0,"marginRight":0,"marginBottom":0,"marginLeft":0}}"#;
        assert!(e.set_page(bad).is_err());
        assert!(e.set_page("{").is_err());
        // The refused settings changed nothing, and an edit keeps the settings.
        e.replace_blocks(0, 1, "[]").unwrap();
        assert!(e.page(0).unwrap().contains(r#""text":"Шапка""#));
    }

    #[test]
    fn pdf_lists_what_it_could_not_draw() {
        let mut e = Engine::new();
        e.add_font(font()).unwrap();
        e.set_document(
            r#"{"version":1,"blocks":[{"type":"leaf","kind":"image","width":50,"height":20,"data":"a.svg"}]}"#,
        )
        .unwrap();
        let (bytes, missing) = e.pdf("Звіт").unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert_eq!(missing, vec!["a.svg"]);
        e.add_svg("a.svg", r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 5 2"><path d="M0 0h5v2z"/></svg>"#);
        assert!(e.pdf("").unwrap().1.is_empty());
        e.clear_pictures();
        assert_eq!(e.pdf("").unwrap().1.len(), 1);
    }

    #[test]
    fn a_rejected_edit_leaves_the_document_alone() {
        let mut e = Engine::new();
        e.set_document(DOC).unwrap();
        assert!(e.replace_blocks(0, 9, "[]").unwrap_err().contains("outside the document"));
        assert!(e.replace_blocks(0, 1, "{").unwrap_err().contains("malformed blocks JSON"));
        assert!(e.replace_blocks(0, 1, r#"[{"type":"paragraph","level":9}]"#).unwrap_err().contains("heading level"));
        assert!(e.set_document(r#"{"version":2,"blocks":[]}"#).unwrap_err().contains("unsupported"));
        assert_eq!(e.text_between(0, 16, "|"), "Перший|Другий");
        assert!(e.add_font(vec![1, 2, 3]).is_err());
    }
}
