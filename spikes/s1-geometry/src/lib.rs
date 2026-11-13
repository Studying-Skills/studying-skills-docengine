//! Spike S1/S4/S6: shaping in Rust (wasm) vs the browser's own text layout; greedy line layout timing.
use std::collections::HashMap;

use rustybuzz::{Face, UnicodeBuffer};
use wasm_bindgen::prelude::*;

/// Advance of `text` in pixels at `size_px` (kerning and default ligatures on, as in browsers).
pub fn advance(face: &Face, text: &str, size_px: f32) -> f32 {
    let mut buf = UnicodeBuffer::new();
    buf.push_str(text);
    let out = rustybuzz::shape(face, &[], buf);
    let units: i32 = out.glyph_positions().iter().map(|p| p.x_advance).sum();
    units as f32 * size_px / face.units_per_em() as f32
}

/// Greedy line breaking of one paragraph: number of lines. `cache` maps a segment to its advance.
pub fn layout_paragraph(face: &Face, text: &str, size_px: f32, width: f32, cache: &mut HashMap<String, f32>) -> usize {
    let mut lines = 1usize;
    let mut x = 0f32;
    let mut start = 0usize;
    for (end, _) in unicode_linebreak::linebreaks(text) {
        let seg = &text[start..end];
        start = end;
        let trimmed = seg.trim_end();
        let (w_full, w_trim) = {
            let mut get = |s: &str| -> f32 {
                if let Some(w) = cache.get(s) {
                    return *w;
                }
                let w = advance(face, s, size_px);
                cache.insert(s.to_owned(), w);
                w
            };
            (get(seg), get(trimmed))
        };
        if x + w_trim > width && x > 0.0 {
            lines += 1;
            x = 0.0;
        }
        x += w_full;
    }
    lines
}

#[wasm_bindgen]
pub struct Engine {
    data: Vec<u8>,
    cache: HashMap<String, f32>,
    para_lines: Vec<usize>,
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new(font: Vec<u8>) -> Engine {
        Engine { data: font, cache: HashMap::new(), para_lines: Vec::new() }
    }

    /// Widths of the words of `line` (split on single spaces), then the width of the whole line.
    pub fn measure_line(&self, line: &str, size_px: f32) -> Vec<f32> {
        let face = Face::from_slice(&self.data, 0).expect("font");
        let mut out: Vec<f32> = line.split(' ').map(|w| advance(&face, w, size_px)).collect();
        out.push(advance(&face, line, size_px));
        out
    }

    /// Full layout of `text` (paragraphs separated by `\n`): total number of lines.
    pub fn layout_all(&mut self, text: &str, size_px: f32, width: f32) -> usize {
        let face = Face::from_slice(&self.data, 0).expect("font");
        self.para_lines = text.split('\n').map(|p| layout_paragraph(&face, p, size_px, width, &mut self.cache)).collect();
        self.para_lines.iter().sum()
    }

    /// Re-layout of one paragraph after an edit (the incremental path): its new line count.
    pub fn relayout(&mut self, index: usize, text: &str, size_px: f32, width: f32) -> usize {
        let face = Face::from_slice(&self.data, 0).expect("font");
        let n = layout_paragraph(&face, text, size_px, width, &mut self.cache);
        if index < self.para_lines.len() {
            self.para_lines[index] = n;
        }
        n
    }

    /// True when the font lacks a glyph for some character of `text` (the browser would fall back to another font).
    pub fn has_notdef(&self, text: &str) -> bool {
        let face = Face::from_slice(&self.data, 0).expect("font");
        let mut buf = UnicodeBuffer::new();
        buf.push_str(text);
        rustybuzz::shape(&face, &[], buf).glyph_infos().iter().any(|g| g.glyph_id == 0)
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }
}
