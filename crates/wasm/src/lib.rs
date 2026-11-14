//! JavaScript bindings. Thin on purpose: see `engine.rs` for the behaviour and its tests.

pub mod engine;

use wasm_bindgen::prelude::*;

fn js<T>(r: Result<T, String>) -> Result<T, JsError> {
    r.map_err(|e| JsError::new(&e))
}

#[wasm_bindgen]
pub struct Engine(engine::Engine, Vec<String>);

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Engine {
        Engine(engine::Engine::new(), Vec::new())
    }

    /// Register a font file; returns `family|bold|italic` as the file states them.
    #[wasm_bindgen(js_name = addFont)]
    pub fn add_font(&mut self, data: Vec<u8>) -> Result<String, JsError> {
        js(self.0.add_font(data))
    }

    #[wasm_bindgen(js_name = fontCount)]
    pub fn font_count(&self) -> usize {
        self.0.font_count()
    }

    /// `[ascent, descent]` of a font (by loading order) as fractions of the font size; empty for an unknown index.
    #[wasm_bindgen(js_name = fontMetrics)]
    pub fn font_metrics(&self, index: u32) -> Vec<f32> {
        self.0.font_metrics(index)
    }

    /// Text as an SVG path in 1/1000 of the font size, y up: `"<advance> <path data>"`; empty when no loaded font
    /// has the characters.
    #[wasm_bindgen(js_name = textOutline)]
    pub fn text_outline(&self, text: &str, family: &str, bold: bool, italic: bool) -> String {
        self.0.text_outline(text, family, bold, italic)
    }

    /// Replace the whole document (engine JSON); returns its content size.
    #[wasm_bindgen(js_name = setDocument)]
    pub fn set_document(&mut self, json: &str) -> Result<u32, JsError> {
        js(self.0.set_document(json))
    }

    /// Replace top-level blocks `from..to` with a JSON array of blocks; returns the new content size.
    #[wasm_bindgen(js_name = replaceBlocks)]
    pub fn replace_blocks(&mut self, from: usize, to: usize, json: &str) -> Result<u32, JsError> {
        js(self.0.replace_blocks(from, to, json))
    }

    #[wasm_bindgen(js_name = contentSize)]
    pub fn content_size(&self) -> u32 {
        self.0.content_size()
    }

    #[wasm_bindgen(js_name = blockCount)]
    pub fn block_count(&self) -> usize {
        self.0.block_count()
    }

    #[wasm_bindgen(js_name = textBetween)]
    pub fn text_between(&self, from: u32, to: u32, block_separator: &str) -> String {
        self.0.text_between(from, to, block_separator)
    }

    /// Page size and margins, header and footer, text settings, styles:
    /// `{"page": {...}, "running": {...}, "text": {...}, "styles": {...}}`; any part may be left out.
    #[wasm_bindgen(js_name = setPage)]
    pub fn set_page(&mut self, json: &str) -> Result<(), JsError> {
        js(self.0.set_page(json))
    }

    /// Lay the document out if it changed; returns one fingerprint per page. A page whose fingerprint is the
    /// same as before looks the same.
    pub fn layout(&mut self) -> Result<Vec<f64>, JsError> {
        js(self.0.layout())
    }

    /// `[width, height]` of a page in points.
    #[wasm_bindgen(js_name = pageSize)]
    pub fn page_size(&self) -> Vec<f32> {
        self.0.page_size()
    }

    /// Display list of page `index` (from 0) as JSON.
    pub fn page(&mut self, index: usize) -> Result<String, JsError> {
        js(self.0.page(index))
    }

    /// Display list of page `index` with spaces, tabs, line breaks and paragraph ends marked (screen only).
    #[wasm_bindgen(js_name = pageMarked)]
    pub fn page_marked(&mut self, index: usize) -> Result<String, JsError> {
        js(self.0.page_marked(index))
    }

    /// Table cells of a page: `x, y, width, height` per cell, a nested table's cells after the cell holding it.
    #[wasm_bindgen(js_name = cellRects)]
    pub fn cell_rects(&mut self, page: usize) -> Result<Vec<f32>, JsError> {
        js(self.0.cell_rects(page))
    }

    /// `[page, x, y, height]` of the caret at `pos`; empty when there is nowhere to put one.
    #[wasm_bindgen(js_name = caretRect)]
    pub fn caret_rect(&mut self, pos: u32) -> Result<Vec<f32>, JsError> {
        js(self.0.caret_rect(pos))
    }

    /// Position nearest to a point of a page; -1 for an empty document.
    #[wasm_bindgen(js_name = hitTest)]
    pub fn hit_test(&mut self, page: usize, x: f32, y: f32) -> Result<i32, JsError> {
        js(self.0.hit_test(page, x, y))
    }

    /// Flat list `page, x, y, width, height` per rectangle of the selection.
    #[wasm_bindgen(js_name = selectionRects)]
    pub fn selection_rects(&mut self, from: u32, to: u32) -> Result<Vec<f32>, JsError> {
        js(self.0.selection_rects(from, to))
    }

    /// Position one line up or down, keeping page x `goal_x`.
    #[wasm_bindgen(js_name = lineMove)]
    pub fn line_move(&mut self, pos: u32, down: bool, goal_x: f32) -> Result<u32, JsError> {
        js(self.0.line_move(pos, down, goal_x))
    }

    /// `[start, end]` of the line `pos` is on.
    #[wasm_bindgen(js_name = lineBounds)]
    pub fn line_bounds(&mut self, pos: u32) -> Result<Vec<u32>, JsError> {
        js(self.0.line_bounds(pos))
    }

    /// A picture file for the PDF: used for objects whose `data` equals `key`.
    #[wasm_bindgen(js_name = addPicture)]
    pub fn add_picture(&mut self, key: &str, bytes: Vec<u8>) {
        self.0.add_picture(key, bytes);
    }

    /// An SVG for the PDF (a formula as outlines, or an SVG picture).
    #[wasm_bindgen(js_name = addSvg)]
    pub fn add_svg(&mut self, key: &str, svg: &str) {
        self.0.add_svg(key, svg);
    }

    #[wasm_bindgen(js_name = clearPictures)]
    pub fn clear_pictures(&mut self) {
        self.0.clear_pictures();
    }

    /// The document as PDF bytes. Objects without a usable picture become empty frames; `pdfMissing` lists them.
    pub fn pdf(&mut self, title: &str) -> Result<Vec<u8>, JsError> {
        let (bytes, missing) = js(self.0.pdf(title))?;
        self.1 = missing;
        Ok(bytes)
    }

    /// `data` of the objects the last `pdf` call could not draw.
    #[wasm_bindgen(js_name = pdfMissing)]
    pub fn pdf_missing(&self) -> Vec<String> {
        self.1.clone()
    }

    /// Column widths of every table in document order: `[columns, width, width, ..., columns, width, ...]`, points.
    #[wasm_bindgen(js_name = tableColumns)]
    pub fn table_columns(&mut self) -> Result<Vec<f32>, JsError> {
        js(self.0.table_columns())
    }

    /// Page (from 0) of a position; -1 for an empty document.
    #[wasm_bindgen(js_name = pageOf)]
    pub fn page_of(&mut self, pos: u32) -> Result<i32, JsError> {
        js(self.0.page_of(pos))
    }
}

/// Version of the document format this build reads.
#[wasm_bindgen(js_name = formatVersion)]
pub fn format_version() -> u32 {
    sfdoc_model::FORMAT_VERSION
}
