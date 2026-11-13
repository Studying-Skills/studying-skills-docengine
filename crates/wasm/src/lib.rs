//! JavaScript bindings. Thin on purpose: see `engine.rs` for the behaviour and its tests.

pub mod engine;

use wasm_bindgen::prelude::*;

fn js<T>(r: Result<T, String>) -> Result<T, JsError> {
    r.map_err(|e| JsError::new(&e))
}

#[wasm_bindgen]
pub struct Engine(engine::Engine);

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Engine {
        Engine(engine::Engine::new())
    }

    /// Register a font file; returns `family|bold|italic` as the file states them.
    #[wasm_bindgen(js_name = addFont)]
    pub fn add_font(&mut self, data: Vec<u8>) -> Result<String, JsError> {
        js(self.0.add_font(data))
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

    /// Lay the document out if it changed; returns the number of pages.
    pub fn layout(&mut self) -> Result<usize, JsError> {
        js(self.0.layout())
    }

    /// Display list of page `index` (from 0) as JSON.
    pub fn page(&mut self, index: usize) -> Result<String, JsError> {
        js(self.0.page(index))
    }
}

/// Version of the document format this build reads.
#[wasm_bindgen(js_name = formatVersion)]
pub fn format_version() -> u32 {
    sfdoc_model::FORMAT_VERSION
}
