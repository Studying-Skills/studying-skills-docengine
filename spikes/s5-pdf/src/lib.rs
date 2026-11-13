//! Spike S5: a PDF with selectable Cyrillic text and subset fonts, built by the same code natively and in wasm.
use std::sync::Arc;

use krilla::Document;
use krilla::geom::Point;
use krilla::page::PageSettings;
use krilla::text::{Font, TextDirection};
use wasm_bindgen::prelude::*;

const PT_PER_MM: f32 = 72.0 / 25.4;

/// A4 pages, DSTU margins (30/10/20/20 mm), 14 pt, 1.5 line spacing; one input line per text line.
/// `fonts` = regular, bold, italic. Every 7th line is bold, every 11th italic.
pub fn build(lines: &[&str], fonts: [Vec<u8>; 3]) -> Result<Vec<u8>, String> {
    let fonts: Vec<Font> = fonts
        .into_iter()
        .map(|d| Font::new(Arc::new(d).into(), 0).ok_or_else(|| "bad font".to_owned()))
        .collect::<Result<_, _>>()?;
    let (w, h) = (210.0 * PT_PER_MM, 297.0 * PT_PER_MM);
    let (left, top, bottom) = (30.0 * PT_PER_MM, 20.0 * PT_PER_MM, 20.0 * PT_PER_MM);
    let size = 14.0f32;
    let leading = size * 1.15 * 1.5;
    let per_page = ((h - top - bottom) / leading).floor() as usize;
    let mut doc = Document::new();
    for chunk in lines.chunks(per_page.max(1)) {
        let mut page = doc.start_page_with(PageSettings::from_wh(w, h).ok_or("page")?);
        let mut surface = page.surface();
        for (i, line) in chunk.iter().enumerate() {
            let font = &fonts[if i % 7 == 6 { 1 } else if i % 11 == 10 { 2 } else { 0 }];
            let y = top + size + leading * i as f32;
            surface.draw_text(Point::from_xy(left, y), font.clone(), size, line, false, TextDirection::Auto);
        }
        surface.finish();
        page.finish();
    }
    doc.finish().map_err(|e| format!("{e:?}"))
}

#[wasm_bindgen]
pub fn build_pdf(text: &str, regular: Vec<u8>, bold: Vec<u8>, italic: Vec<u8>) -> Result<Vec<u8>, JsError> {
    let lines: Vec<&str> = text.split('\n').collect();
    build(&lines, [regular, bold, italic]).map_err(|e| JsError::new(&e))
}
