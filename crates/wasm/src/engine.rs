//! The engine as the editor drives it: fonts in, document in, edits in, pages out. No JavaScript types here, so
//! the whole flow is tested natively.

use serde::Serialize;
use sfdoc_fonts::{Font, FontSet};
use sfdoc_geom::{Point, Rect, Size};
use sfdoc_layout::{Item, Layout, Layouter, Object, Page};
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

    /// How many fonts are loaded. Display lists name fonts by their index in loading order.
    pub fn font_count(&self) -> usize {
        self.fonts.len()
    }

    /// `[ascent, descent]` of font `index` as fractions of the font size: a painter that places text by the top
    /// of its box needs them to hit the baseline the layout chose.
    pub fn font_metrics(&self, index: u32) -> Vec<f32> {
        self.fonts.get(sfdoc_fonts::FontId(index)).map_or_else(Vec::new, |f| {
            let m = f.metrics();
            vec![m.ascent / m.units_per_em, m.descent / m.units_per_em]
        })
    }

    /// Text as an outline, for drawings that must not depend on the reader's fonts (words inside formulas): an SVG
    /// path in units of 1/1000 of the font size, y up, starting at the origin, with the advance in the same units
    /// in front: `"<advance> <path data>"`. Empty when no loaded font has one of the characters.
    pub fn text_outline(&self, text: &str, family: &str, bold: bool, italic: bool) -> String {
        struct Path {
            d: String,
            dx: f32,
            k: f32,
        }
        impl Path {
            fn xy(&mut self, x: f32, y: f32) {
                use std::fmt::Write as _;
                let _ = write!(
                    self.d,
                    "{} {}",
                    ((x + self.dx) * self.k * 10.0).round() / 10.0,
                    (y * self.k * 10.0).round() / 10.0
                );
            }
            fn cmd(&mut self, c: char, pts: &[(f32, f32)]) {
                self.d.push(c);
                for (i, (x, y)) in pts.iter().enumerate() {
                    if i > 0 {
                        self.d.push(' ');
                    }
                    self.xy(*x, *y);
                }
            }
        }
        impl sfdoc_fonts::OutlineSink for Path {
            fn move_to(&mut self, x: f32, y: f32) {
                self.cmd('M', &[(x, y)]);
            }
            fn line_to(&mut self, x: f32, y: f32) {
                self.cmd('L', &[(x, y)]);
            }
            fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
                self.cmd('Q', &[(x1, y1), (x, y)]);
            }
            fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
                self.cmd('C', &[(x1, y1), (x2, y2), (x, y)]);
            }
            fn close(&mut self) {
                self.d.push('Z');
            }
        }
        let family = (!family.is_empty()).then_some(family);
        let Some(wanted) = self.fonts.resolve(family, bold, italic) else { return String::new() };
        // One font for the whole piece: the wanted one, or the first that has every character.
        let covers = |id: sfdoc_fonts::FontId| self.fonts.get(id).is_some_and(|f| text.chars().all(|c| f.covers(c)));
        let id = if covers(wanted.id) {
            wanted.id
        } else {
            match text.chars().find_map(|c| self.fonts.fallback_for(c, bold, italic).filter(|id| covers(*id))) {
                Some(id) => id,
                None => return String::new(),
            }
        };
        let Some(font) = self.fonts.get(id) else { return String::new() };
        let Ok(shaper) = font.shaper() else { return String::new() };
        let mut path = Path { d: String::new(), dx: 0.0, k: 1000.0 / font.metrics().units_per_em };
        let mut pen = 0.0f32;
        for g in shaper.shape(text) {
            path.dx = pen + g.x_offset;
            shaper.outline(g.id, &mut path);
            pen += g.x_advance;
        }
        format!("{} {}", (pen * path.k * 10.0).round() / 10.0, path.d)
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

    /// Page size, margins, running header/footer, text settings and styles as JSON:
    /// `{"page": {...}, "running": {...}, "text": {...}, "styles": {...}}`; any part may be left out. Refused
    /// settings change nothing.
    pub fn set_page(&mut self, json: &str) -> Result<(), String> {
        #[derive(serde::Deserialize)]
        struct Settings {
            page: Option<sfdoc_model::PageSetup>,
            running: Option<sfdoc_model::Running>,
            text: Option<sfdoc_model::TextSetup>,
            styles: Option<sfdoc_model::Styles>,
        }
        let s: Settings = serde_json::from_str(json).map_err(|e| format!("malformed page settings: {e}"))?;
        let next = Document {
            version: self.doc.version,
            page: s.page.unwrap_or(self.doc.page),
            running: s.running.unwrap_or_else(|| self.doc.running.clone()),
            text: s.text.unwrap_or(self.doc.text),
            styles: s.styles.unwrap_or_else(|| self.doc.styles.clone()),
            blocks: Vec::new(),
        };
        next.validate().map_err(|e| e.to_string())?;
        let d = &self.doc;
        if next.page != d.page || next.running != d.running || next.text != d.text || next.styles != d.styles {
            self.doc.page = next.page;
            self.doc.running = next.running;
            self.doc.text = next.text;
            self.doc.styles = next.styles;
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

    /// The display list of page `index` (from 0) as JSON, in the form the screen needs (see [`screen_json`]).
    pub fn page(&mut self, index: usize) -> Result<String, String> {
        self.ensure_layout()?;
        let Some(layout) = self.layout.as_ref() else { return Err("layout is missing".into()) };
        let page = layout.page(index, &self.fonts).ok_or_else(|| format!("page {index} of {}", layout.pages.len()))?;
        screen_json(&page)
    }

    /// The display list of page `index` with the marks of what is typed but not printed (spaces, tabs, line
    /// breaks, paragraph ends). For the screen: the PDF never has them.
    pub fn page_marked(&mut self, index: usize) -> Result<String, String> {
        self.ensure_layout()?;
        let Some(layout) = self.layout.as_ref() else { return Err("layout is missing".into()) };
        let page = layout
            .page_with(index, &self.fonts, true)
            .ok_or_else(|| format!("page {index} of {}", layout.pages.len()))?;
        screen_json(&page)
    }

    /// Table cells of a page as a flat list: `x, y, width, height` for every cell.
    pub fn cell_rects(&mut self, page: usize) -> Result<Vec<f32>, String> {
        Ok(self.ensure_layout()?.cell_rects(page).iter().flat_map(|r| [r.x, r.y, r.w, r.h]).collect())
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

    /// Column widths of every table in document order, flattened: the number of columns, then that many widths
    /// in points, for each table.
    pub fn table_columns(&mut self) -> Result<Vec<f32>, String> {
        self.ensure_layout()?;
        Ok(self.layouter.table_columns().into_iter().flat_map(|c| std::iter::once(c.len() as f32).chain(c)).collect())
    }

    /// Page (from 0) a position is on; -1 when the document is empty.
    pub fn page_of(&mut self, pos: u32) -> Result<i32, String> {
        Ok(self.ensure_layout()?.page_of(pos).map_or(-1, |p| p as i32))
    }
}

/// A page as the screen paints it. The browser draws the text of a run itself, so the glyphs stay out: they
/// are nine tenths of the full display list, and the editor reads a page on every keystroke. What the screen
/// still needs from them is where the visible part of a run ends when it has trailing spaces (`ink`).
#[derive(Serialize)]
struct ScreenPage<'a> {
    size: Size,
    items: Vec<ScreenItem<'a>>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum ScreenItem<'a> {
    Rect {
        rect: Rect,
        color: sfdoc_model::Color,
    },
    Glyphs {
        font: u32,
        size: f32,
        origin: Point,
        color: sfdoc_model::Color,
        text: &'a str,
        width: f32,
        /// Width up to the first trailing space; absent when the run has none.
        #[serde(skip_serializing_if = "Option::is_none")]
        ink: Option<f32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        link: Option<&'a str>,
    },
    Object(&'a Object),
}

fn screen_json(page: &Page) -> Result<String, String> {
    let items = page
        .items
        .iter()
        .map(|item| match item {
            Item::Rect { rect, color } => ScreenItem::Rect { rect: *rect, color: *color },
            Item::Glyphs(run) => {
                let spaces = run.text.chars().rev().take_while(|c| *c == ' ').count();
                // The glyphs of the trailing spaces come last: the visible part ends where the first begins.
                let ink = (spaces > 0)
                    .then(|| run.glyphs.len().checked_sub(spaces).and_then(|i| run.glyphs.get(i)).map(|g| g.x))
                    .flatten();
                ScreenItem::Glyphs {
                    font: run.font,
                    size: run.size,
                    origin: run.origin,
                    color: run.color,
                    text: &run.text,
                    width: run.width,
                    ink,
                    link: run.link.as_deref(),
                }
            }
            Item::Object(object) => ScreenItem::Object(object),
        })
        .collect();
    serde_json::to_string(&ScreenPage { size: page.size, items }).map_err(|e| e.to_string())
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
        assert_eq!((e.font_count(), e.font_metrics(0)), (1, vec![1825.0 / 2048.0, 443.0 / 2048.0]));
        assert!(e.font_metrics(1).is_empty());
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
    fn the_screen_page_has_the_text_of_a_run_without_its_glyphs() {
        let mut e = Engine::new();
        e.add_font(font()).unwrap();
        e.set_document(
            r#"{"version":1,"blocks":[{"type":"paragraph","inlines":[{"type":"text","text":"Два слова"}]}]}"#,
        )
        .unwrap();
        let page: serde_json::Value = serde_json::from_str(&e.page(0).unwrap()).unwrap();
        let runs: Vec<&serde_json::Value> =
            page["items"].as_array().unwrap().iter().filter(|i| i["type"] == "glyphs").collect();
        assert_eq!(runs.iter().filter(|r| r.get("ink").is_some()).count(), 1);
        for run in &runs {
            assert!(run.get("glyphs").is_none());
            assert!(run["text"].is_string() && run["width"].is_number() && run["origin"]["x"].is_number());
            let text = run["text"].as_str().unwrap();
            // A run that ends with spaces says where its visible part ends; others do not carry the field.
            match run.get("ink") {
                Some(ink) => {
                    assert!(text.ends_with(' '));
                    let ink = ink.as_f64().unwrap();
                    assert!(ink > 0.0 && ink < run["width"].as_f64().unwrap());
                }
                None => assert!(!text.ends_with(' ')),
            }
        }
        assert_eq!(page["size"]["w"].as_f64().unwrap().round(), 595.0);
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
    fn text_becomes_an_outline_with_its_advance() {
        let mut e = Engine::new();
        assert_eq!(e.text_outline("а", "", false, false), "");
        e.add_font(font()).unwrap();
        let one = e.text_outline("с", "serif", false, false);
        let (advance, d) = one.split_once(' ').unwrap();
        // Liberation Serif: «с» is 0.444 em wide.
        assert!((advance.parse::<f32>().unwrap() - 443.8).abs() < 0.2, "{advance}");
        assert!(d.starts_with('M') && d.ends_with('Z') && d.contains('Q'));
        // Two letters: the second outline starts after the first one's advance.
        let two = e.text_outline("сс", "", false, false);
        assert!((two.split_once(' ').unwrap().0.parse::<f32>().unwrap() - 887.7).abs() < 0.3);
        assert_eq!(two.matches('M').count(), 2 * d.matches('M').count());
        // A space has an advance and no outline; a character no font has gives nothing.
        assert_eq!(e.text_outline(" ", "", false, false), "250 ");
        assert_eq!(e.text_outline("語", "", false, false), "");
        // The PDF's formula reader takes the path as it is.
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 -800 1000 1000"><g transform="scale(1,-1)"><path d="{d}"/></g></svg>"#
        );
        assert!(sfdoc_pdf::outline_svg::read(&svg).is_some());
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
    fn styles_hyphenation_marks_and_cells_reach_the_editor() {
        let mut e = Engine::new();
        e.add_font(font()).unwrap();
        let long = "сортування ".repeat(40);
        let doc = format!(
            r#"{{"version":1,"blocks":[{{"type":"paragraph","inlines":[{{"type":"text","text":"{long}"}}]}},{{"type":"table","borders":"outer","rows":[{{"cells":[{{"blocks":[{{"type":"paragraph"}}]}},{{"blocks":[{{"type":"paragraph"}}]}}]}}]}}]}}"#
        );
        e.set_document(&doc).unwrap();
        let plain = e.page(0).unwrap();
        assert!(!plain.contains("-\""));
        // Hyphenation and a smaller body font: both change the page, and both survive an edit.
        e.set_page(r#"{"text":{"hyphenate":true},"styles":{"body":{"size":10}}}"#).unwrap();
        let hyphenated = e.page(0).unwrap();
        assert!(hyphenated.contains("-\"") && hyphenated.contains(r#""size":10.0"#), "{hyphenated}");
        e.replace_blocks(1, 2, "[]").unwrap();
        assert!(e.page(0).unwrap().contains("-\""));
        assert!(e.set_page(r#"{"styles":{"body":{"size":-3}}}"#).is_err());
        // A full document without settings of its own keeps none: the editor sets them again after it.
        e.set_document(&doc).unwrap();
        assert_eq!(e.page(0).unwrap(), plain);

        // Two cells, four numbers each; marks only on request.
        let cells = e.cell_rects(0).unwrap();
        assert_eq!(cells.len(), 8);
        assert!(cells[4] > cells[0] && cells[1] == cells[5]);
        assert!(e.cell_rects(7).unwrap().is_empty());
        assert!(!plain.contains('¶'));
        assert!(e.page_marked(0).unwrap().contains('¶'));
        assert_eq!(e.page_marked(9).unwrap_err(), "page 9 of 1");
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
