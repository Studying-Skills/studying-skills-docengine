//! Rasterise a page of the display list. Used by snapshot tests and `sfdoc render`; the browser paints with the
//! DOM and the export writes PDF, so nothing user-facing depends on this crate's antialiasing.

use sfdoc_fonts::{FontId, FontSet, OutlineSink, Shaper};
use sfdoc_layout::{GlyphRun, Item, Page};
use sfdoc_model::Color;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RasterError {
    #[error("page of {0}x{1} px cannot be rasterised")]
    BadSize(u32, u32),
    #[error("the display list refers to font {0}, which is not loaded")]
    MissingFont(u32),
    #[error("PNG encoding failed")]
    Encode,
}

/// Largest side of a raster in pixels: a guard against absurd scales, not a real limit.
const MAX_SIDE: u32 = 16_384;

struct PathSink<'a> {
    pb: &'a mut PathBuilder,
    /// Font units → pixels, and the glyph's origin in pixels.
    k: f32,
    ox: f32,
    oy: f32,
}

impl OutlineSink for PathSink<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(self.ox + x * self.k, self.oy - y * self.k);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(self.ox + x * self.k, self.oy - y * self.k);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.pb.quad_to(self.ox + x1 * self.k, self.oy - y1 * self.k, self.ox + x * self.k, self.oy - y * self.k);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (k, ox, oy) = (self.k, self.ox, self.oy);
        self.pb.cubic_to(ox + x1 * k, oy - y1 * k, ox + x2 * k, oy - y2 * k, ox + x * k, oy - y * k);
    }
    fn close(&mut self) {
        self.pb.close();
    }
}

fn paint(color: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(color.r, color.g, color.b, 255);
    p.anti_alias = true;
    p
}

fn draw_run(pixmap: &mut Pixmap, run: &GlyphRun, shaper: &Shaper<'_>, upm: f32, scale: f32) {
    let mut pb = PathBuilder::new();
    let k = run.size / upm * scale;
    for g in &run.glyphs {
        let mut sink = PathSink { pb: &mut pb, k, ox: (run.origin.x + g.x) * scale, oy: (run.origin.y + g.y) * scale };
        shaper.outline(g.id, &mut sink);
    }
    if let Some(path) = pb.finish() {
        pixmap.fill_path(&path, &paint(run.color), FillRule::Winding, Transform::identity(), None);
    }
}

/// Render one page on a white background. `scale` is pixels per point (96 dpi = 4/3, 192 dpi = 8/3).
pub fn render_page(page: &Page, fonts: &FontSet, scale: f32) -> Result<Pixmap, RasterError> {
    let (w, h) = ((page.size.w * scale).ceil(), (page.size.h * scale).ceil());
    let bad = || RasterError::BadSize(w.max(0.0) as u32, h.max(0.0) as u32);
    if !(w.is_finite() && h.is_finite()) || w < 1.0 || h < 1.0 || w > MAX_SIDE as f32 || h > MAX_SIDE as f32 {
        return Err(bad());
    }
    let mut pixmap = Pixmap::new(w as u32, h as u32).ok_or_else(bad)?;
    pixmap.fill(tiny_skia::Color::WHITE);
    for item in &page.items {
        match item {
            Item::Rect { rect, color } => {
                if let Some(r) = Rect::from_xywh(rect.x * scale, rect.y * scale, rect.w * scale, rect.h * scale) {
                    pixmap.fill_rect(r, &paint(*color), Transform::identity(), None);
                }
            }
            Item::Glyphs(run) => {
                let font = fonts.get(FontId(run.font)).ok_or(RasterError::MissingFont(run.font))?;
                let shaper = font.shaper().map_err(|_| RasterError::MissingFont(run.font))?;
                draw_run(&mut pixmap, run, &shaper, font.metrics().units_per_em, scale);
            }
            // Pictures and formulas are drawn by whoever has them; here a framed grey box marks their place.
            Item::Object(o) => {
                let r = o.rect;
                if let Some(outer) = Rect::from_xywh(r.x * scale, r.y * scale, r.w * scale, r.h * scale) {
                    pixmap.fill_rect(outer, &paint(Color::rgb(150, 150, 150)), Transform::identity(), None);
                }
                if let Some(inner) =
                    Rect::from_xywh(r.x * scale + 1.0, r.y * scale + 1.0, r.w * scale - 2.0, r.h * scale - 2.0)
                {
                    pixmap.fill_rect(inner, &paint(Color::rgb(235, 235, 235)), Transform::identity(), None);
                }
            }
        }
    }
    Ok(pixmap)
}

/// [`render_page`] encoded as PNG.
pub fn render_png(page: &Page, fonts: &FontSet, scale: f32) -> Result<Vec<u8>, RasterError> {
    render_page(page, fonts, scale)?.encode_png().map_err(|_| RasterError::Encode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sfdoc_fonts::Font;
    use sfdoc_geom::{Point, Size};
    use sfdoc_layout::PlacedGlyph;

    fn fonts() -> FontSet {
        let mut set = FontSet::new();
        let path = format!("{}/../../assets/fonts/liberation/LiberationSerif-Regular.ttf", env!("CARGO_MANIFEST_DIR"));
        set.add_described(Font::from_bytes(std::fs::read(path).unwrap()).unwrap()).unwrap();
        set
    }

    /// Bounding box of pixels darker than mid-grey: (left, top, right, bottom), or None for a blank raster.
    fn ink(p: &Pixmap) -> Option<(u32, u32, u32, u32)> {
        let mut b: Option<(u32, u32, u32, u32)> = None;
        for y in 0..p.height() {
            for x in 0..p.width() {
                let c = p.pixel(x, y).unwrap();
                if u32::from(c.red()) + u32::from(c.green()) + u32::from(c.blue()) < 384 {
                    b = Some(b.map_or((x, y, x, y), |(l, t, r, bo)| (l.min(x), t.min(y), r.max(x), bo.max(y))));
                }
            }
        }
        b
    }

    fn run(text: &str, size: f32, origin: Point, set: &FontSet) -> GlyphRun {
        let font = set.get(FontId(0)).unwrap();
        let k = size / font.metrics().units_per_em;
        let mut x = 0.0;
        let glyphs = font
            .shaper()
            .unwrap()
            .shape(text)
            .iter()
            .map(|g| {
                let p = PlacedGlyph { id: g.id, cluster: g.cluster, x: x + g.x_offset * k, y: -g.y_offset * k };
                x += g.x_advance * k;
                p
            })
            .collect();
        GlyphRun { font: 0, size, origin, color: Color::BLACK, glyphs, text: text.into(), width: x, link: None }
    }

    #[test]
    fn a_blank_page_is_white_and_sized_by_the_scale() {
        let page = Page { size: Size { w: 100.0, h: 50.0 }, items: vec![] };
        let p = render_page(&page, &fonts(), 2.0).unwrap();
        assert_eq!((p.width(), p.height()), (200, 100));
        assert_eq!(ink(&p), None);
        assert!(render_png(&page, &fonts(), 1.0).unwrap().starts_with(b"\x89PNG"));
    }

    #[test]
    fn glyphs_land_on_their_baseline_within_their_width() {
        let set = fonts();
        // "НН" has no descenders and no overshoot: its ink sits between the cap height and the baseline.
        let r = run("НН", 40.0, Point { x: 10.0, y: 60.0 }, &set);
        let width = r.width;
        let page = Page { size: Size { w: 120.0, h: 80.0 }, items: vec![Item::Glyphs(r)] };
        let (l, t, rt, b) = ink(&render_page(&page, &set, 1.0).unwrap()).unwrap();
        let cap = set.get(FontId(0)).unwrap().metrics().cap_height * 40.0 / 2048.0;
        assert!((b as f32 - 59.0).abs() <= 1.0, "bottom {b}");
        assert!((t as f32 - (60.0 - cap)).abs() <= 1.5, "top {t}, cap height {cap}");
        assert!(l >= 10 && (rt as f32) <= 10.0 + width + 1.0, "ink {l}..{rt}, run width {width}");
    }

    #[test]
    fn rectangles_and_colours() {
        let page = Page {
            size: Size { w: 20.0, h: 20.0 },
            items: vec![Item::Rect { rect: sfdoc_geom::Rect::new(5.0, 5.0, 10.0, 4.0), color: Color::rgb(200, 0, 0) }],
        };
        let p = render_page(&page, &fonts(), 1.0).unwrap();
        let c = p.pixel(10, 7).unwrap();
        assert_eq!((c.red(), c.green(), c.blue()), (200, 0, 0));
        assert_eq!(p.pixel(10, 12).unwrap().red(), 255);
    }

    #[test]
    fn bad_input_is_an_error() {
        let set = fonts();
        let huge = Page { size: Size { w: 1e9, h: 10.0 }, items: vec![] };
        assert!(matches!(render_page(&huge, &set, 1.0), Err(RasterError::BadSize(..))));
        let nan = Page { size: Size { w: f32::NAN, h: 10.0 }, items: vec![] };
        assert!(matches!(render_page(&nan, &set, 1.0), Err(RasterError::BadSize(..))));
        let mut r = run("а", 10.0, Point { x: 0.0, y: 10.0 }, &set);
        r.font = 5;
        let page = Page { size: Size { w: 20.0, h: 20.0 }, items: vec![Item::Glyphs(r)] };
        assert_eq!(render_page(&page, &set, 1.0).unwrap_err(), RasterError::MissingFont(5));
    }
}
