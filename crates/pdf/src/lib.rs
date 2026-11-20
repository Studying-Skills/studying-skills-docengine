//! PDF from the display list.
//!
//! The writer places the very glyphs the layout chose, at the very coordinates, so the file and the screen cannot
//! disagree about line or page breaks. Text stays text (selectable, searchable), fonts are embedded as subsets.

pub mod outline_svg;

use std::collections::HashMap;
use std::sync::Arc;

use krilla::Document;
use krilla::action::{Action, LinkAction};
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::image::Image;
use krilla::metadata::Metadata;
use krilla::page::PageSettings;
use krilla::paint::Fill;
use krilla::surface::Surface;
use krilla::text::{Font, GlyphId, KrillaGlyph};
use sfdoc_fonts::{FontId, FontSet};
use sfdoc_layout::{GlyphRun, Item, Layout, Object, ObjectKind};
use sfdoc_model::Color;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PdfError {
    #[error("the display list names font {0}, which is not loaded")]
    MissingFont(u32),
    #[error("font {0} cannot be embedded")]
    BadFont(u32),
    #[error("page size {0}×{1} pt is not usable")]
    BadPage(f32, f32),
    #[error("the PDF could not be written: {0}")]
    Write(String),
}

/// What an object of the display list looks like. The engine does not load pictures or typeset formulas itself.
pub enum Picture {
    /// PNG, JPEG, GIF or WebP file contents.
    Raster(Vec<u8>),
    /// An SVG document; text inside it must already be outlines.
    Svg(String),
}

/// Supplies pictures and formulas. `data` is what the document holds: an image path or a formula source.
pub trait Assets {
    fn picture(&self, kind: ObjectKind, data: &str) -> Option<Picture>;
}

/// No pictures at all: every object becomes a framed empty box.
pub struct NoAssets;

impl Assets for NoAssets {
    fn picture(&self, _: ObjectKind, _: &str) -> Option<Picture> {
        None
    }
}

impl<S: std::hash::BuildHasher> Assets for HashMap<String, Picture, S> {
    fn picture(&self, _: ObjectKind, data: &str) -> Option<Picture> {
        match self.get(data)? {
            Picture::Raster(b) => Some(Picture::Raster(b.clone())),
            Picture::Svg(s) => Some(Picture::Svg(s.clone())),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Info {
    pub title: Option<String>,
    pub author: Option<String>,
    /// BCP 47 language of the text, e.g. `uk`.
    pub language: Option<String>,
    /// Write an uncompressed file a person (or a test) can read. Larger; not for users.
    pub readable: bool,
}

/// What went wrong without stopping the export: the file is complete, these objects are empty frames in it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub missing: Vec<String>,
}

fn fill(c: Color) -> Fill {
    Fill { paint: rgb::Color::new(c.r, c.g, c.b).into(), ..Fill::default() }
}

fn rect_path(x: f32, y: f32, w: f32, h: f32) -> Option<krilla::geom::Path> {
    let mut b = PathBuilder::new();
    b.push_rect(Rect::from_xywh(x, y, w, h)?);
    b.finish()
}

fn image_of(bytes: Vec<u8>) -> Option<Image> {
    let header: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
    let data: krilla::Data = Arc::new(bytes).into();
    let made = match &header {
        [0x89, b'P', b'N', b'G'] => Image::from_png(data, true),
        [0xff, 0xd8, ..] => Image::from_jpeg(data, true),
        [b'G', b'I', b'F', _] => Image::from_gif(data, true),
        [b'R', b'I', b'F', b'F'] => Image::from_webp(data, true),
        _ => return None,
    };
    made.ok()
}

/// Formula-style SVG (filled outlines) through the built-in reader; with the `svg` feature, any other SVG through
/// the full renderer.
fn draw_svg(s: &mut Surface<'_>, svg: &str, r: sfdoc_geom::Rect) -> Option<()> {
    if let Some(d) = outline_svg::read(svg) {
        let [vx, vy, vw, vh] = d.view;
        let (kx, ky) = (r.w / vw, r.h / vh);
        let at = |x: f32, y: f32| (r.x + (x - vx) * kx, r.y + (y - vy) * ky);
        for shape in &d.shapes {
            let mut b = PathBuilder::new();
            for seg in &shape.segs {
                match *seg {
                    outline_svg::Seg::Move(x, y) => {
                        let p = at(x, y);
                        b.move_to(p.0, p.1);
                    }
                    outline_svg::Seg::Line(x, y) => {
                        let p = at(x, y);
                        b.line_to(p.0, p.1);
                    }
                    outline_svg::Seg::Quad(x1, y1, x, y) => {
                        let (c, p) = (at(x1, y1), at(x, y));
                        b.quad_to(c.0, c.1, p.0, p.1);
                    }
                    outline_svg::Seg::Cubic(x1, y1, x2, y2, x, y) => {
                        let (c1, c2, p) = (at(x1, y1), at(x2, y2), at(x, y));
                        b.cubic_to(c1.0, c1.1, c2.0, c2.1, p.0, p.1);
                    }
                    outline_svg::Seg::Close => b.close(),
                }
            }
            if let Some(p) = b.finish() {
                s.set_fill(Some(fill(shape.color)));
                s.draw_path(&p);
            }
        }
        return Some(());
    }
    full_svg(s, svg, r)
}

#[cfg(feature = "svg")]
fn full_svg(s: &mut Surface<'_>, svg: &str, r: sfdoc_geom::Rect) -> Option<()> {
    use krilla_svg::{SurfaceExt, SvgSettings};
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).ok()?;
    s.push_transform(&Transform::from_translate(r.x, r.y));
    let done = s.draw_svg(&tree, Size::from_wh(r.w, r.h)?, SvgSettings { embed_text: false, ..Default::default() });
    s.pop();
    done
}

#[cfg(not(feature = "svg"))]
fn full_svg(_: &mut Surface<'_>, _: &str, _: sfdoc_geom::Rect) -> Option<()> {
    None
}

struct Writer<'a> {
    fonts: &'a FontSet,
    assets: &'a dyn Assets,
    embedded: HashMap<u32, Font>,
    report: Report,
}

impl Writer<'_> {
    fn font(&mut self, id: u32) -> Result<Font, PdfError> {
        if let Some(f) = self.embedded.get(&id) {
            return Ok(f.clone());
        }
        let font = self.fonts.get(FontId(id)).ok_or(PdfError::MissingFont(id))?;
        let made = Font::new(Arc::clone(font.data()).into(), font.index()).ok_or(PdfError::BadFont(id))?;
        self.embedded.insert(id, made.clone());
        Ok(made)
    }

    fn run(&mut self, s: &mut Surface<'_>, run: &GlyphRun) -> Result<(), PdfError> {
        let font = self.font(run.font)?;
        let size = run.size.max(0.01);
        // A cluster's text ends where the next cluster (in text order) starts.
        let mut starts: Vec<u32> = run.glyphs.iter().map(|g| g.cluster).collect();
        starts.sort_unstable();
        starts.dedup();
        let end_of = |c: u32| -> usize {
            let next = starts.partition_point(|s| *s <= c);
            starts.get(next).map_or(run.text.len(), |n| *n as usize)
        };
        let glyphs: Vec<KrillaGlyph> = run
            .glyphs
            .iter()
            .enumerate()
            .map(|(i, g)| {
                let next = run.glyphs.get(i + 1).map_or(run.width, |n| n.x);
                let start = (g.cluster as usize).min(run.text.len());
                let end = end_of(g.cluster).clamp(start, run.text.len());
                KrillaGlyph::new(
                    GlyphId::new(u32::from(g.id)),
                    (next - g.x) / size,
                    0.0,
                    -g.y / size,
                    0.0,
                    start..end,
                    None,
                )
            })
            .collect();
        let first = run.glyphs.first().map_or(0.0, |g| g.x);
        s.set_fill(Some(fill(run.color)));
        s.draw_glyphs(Point::from_xy(run.origin.x + first, run.origin.y), &glyphs, font, &run.text, size, false);
        Ok(())
    }

    fn object(&mut self, s: &mut Surface<'_>, o: &Object) {
        let r = o.rect;
        let Some(size) = Size::from_wh(r.w, r.h) else { return };
        let picture = o.data.as_deref().and_then(|d| self.assets.picture(o.kind, d));
        let drawn = match picture {
            Some(Picture::Raster(bytes)) => image_of(bytes).map(|img| {
                s.push_transform(&Transform::from_translate(r.x, r.y));
                s.draw_image(img, size);
                s.pop();
            }),
            Some(Picture::Svg(svg)) => draw_svg(s, &svg, r),
            None => None,
        };
        if drawn.is_none() {
            self.report.missing.push(o.data.clone().unwrap_or_default());
            // A hairline frame, so the reader sees that something belongs here.
            let grey = Color::rgb(150, 150, 150);
            s.set_fill(Some(fill(grey)));
            for (x, y, w, h) in [
                (r.x, r.y, r.w, 0.5),
                (r.x, r.bottom() - 0.5, r.w, 0.5),
                (r.x, r.y, 0.5, r.h),
                (r.right() - 0.5, r.y, 0.5, r.h),
            ] {
                if let Some(p) = rect_path(x, y, w, h) {
                    s.draw_path(&p);
                }
            }
        }
    }
}

/// Write the whole layout as a PDF.
pub fn write_pdf(
    layout: &Layout,
    fonts: &FontSet,
    assets: &dyn Assets,
    info: &Info,
) -> Result<(Vec<u8>, Report), PdfError> {
    let mut w = Writer { fonts, assets, embedded: HashMap::new(), report: Report::default() };
    let mut settings = krilla::SerializeSettings::default();
    if info.readable {
        settings.pretty = true;
        settings.compress_content_streams = false;
    }
    let mut doc = Document::new_with(settings);
    let size = layout.page_size();
    for index in 0..layout.pages.len() {
        let Some(list) = layout.page(index, fonts) else { continue };
        let settings = PageSettings::from_wh(size.w, size.h).ok_or(PdfError::BadPage(size.w, size.h))?;
        let mut page = doc.start_page_with(settings);
        let mut links: Vec<(Rect, String)> = Vec::new();
        {
            let mut s = page.surface();
            for item in &list.items {
                match item {
                    Item::Rect { rect, color } => {
                        if let Some(p) = rect_path(rect.x, rect.y, rect.w, rect.h) {
                            s.set_fill(Some(fill(*color)));
                            s.draw_path(&p);
                        }
                    }
                    Item::Glyphs(run) => {
                        w.run(&mut s, run)?;
                        if let Some(url) = &run.link
                            && let Some(r) =
                                Rect::from_xywh(run.origin.x, run.origin.y - run.size * 0.8, run.width, run.size)
                        {
                            links.push((r, url.clone()));
                        }
                    }
                    Item::Object(o) => w.object(&mut s, o),
                }
            }
            s.finish();
        }
        for (rect, url) in links {
            let target = Target::Action(Action::Link(LinkAction::new(url)));
            page.add_annotation(Annotation::new_link(LinkAnnotation::new(rect, target), None));
        }
        page.finish();
    }
    let mut meta = Metadata::new().creator("StudyFlow".to_owned());
    if let Some(t) = &info.title {
        meta = meta.title(t.clone());
    }
    if let Some(a) = &info.author {
        meta = meta.authors(vec![a.clone()]);
    }
    if let Some(l) = &info.language {
        meta = meta.language(l.clone());
    }
    doc.set_metadata(meta);
    let bytes = doc.finish().map_err(|e| PdfError::Write(format!("{e:?}")))?;
    Ok((bytes, w.report))
}

#[cfg(test)]
mod tests;
