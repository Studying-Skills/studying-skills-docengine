//! Fonts: loading from bytes, metrics, shaping, glyph outlines and the fallback chain.
//!
//! The engine never reads system fonts. Whoever embeds it hands over font files as bytes, so the browser, the
//! server and the tests all measure text with exactly the same data.

use std::collections::HashMap;
use std::sync::Arc;

use rustybuzz::ttf_parser::{self, GlyphId};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FontError {
    #[error("not a readable TrueType or OpenType font")]
    Unreadable,
}

/// Font-wide measurements in font units (divide by `units_per_em` and multiply by the size to get points).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub units_per_em: f32,
    /// Distance from the baseline to the top of the line box (positive).
    pub ascent: f32,
    /// Distance from the baseline to the bottom of the line box (positive).
    pub descent: f32,
    pub line_gap: f32,
    pub x_height: f32,
    pub cap_height: f32,
    /// Offset of the underline's centre below the baseline (positive = below) and its thickness.
    pub underline_offset: f32,
    pub underline_thickness: f32,
    /// Offset of the strike-through's centre above the baseline and its thickness.
    pub strikeout_offset: f32,
    pub strikeout_thickness: f32,
}

impl Metrics {
    /// Height of a single-spaced line at `size` points: ascent + descent + line gap.
    pub fn line_height(&self, size: f32) -> f32 {
        (self.ascent + self.descent + self.line_gap) * size / self.units_per_em
    }
    pub fn scale(&self, units: f32, size: f32) -> f32 {
        units * size / self.units_per_em
    }
}

/// One font face, parsed once for its metrics; the bytes are shared, never copied.
#[derive(Clone, Debug)]
pub struct Font {
    data: Arc<Vec<u8>>,
    index: u32,
    metrics: Metrics,
    /// Sorted, non-overlapping ranges of the code points the font has glyphs for.
    coverage: Arc<Vec<(u32, u32)>>,
}

impl Font {
    pub fn from_bytes(data: Vec<u8>) -> Result<Font, FontError> {
        Self::from_shared(Arc::new(data), 0)
    }

    pub fn from_shared(data: Arc<Vec<u8>>, index: u32) -> Result<Font, FontError> {
        let face = ttf_parser::Face::parse(&data, index).map_err(|_| FontError::Unreadable)?;
        let upm = f32::from(face.units_per_em());
        if upm <= 0.0 {
            return Err(FontError::Unreadable);
        }
        let ascent = f32::from(face.ascender());
        let descent = -f32::from(face.descender());
        let under = face.underline_metrics();
        let strike = face.strikeout_metrics();
        let x_height = face.x_height().map_or(upm * 0.5, f32::from);
        let metrics = Metrics {
            units_per_em: upm,
            ascent,
            descent,
            line_gap: f32::from(face.line_gap()),
            x_height,
            cap_height: face.capital_height().map_or(upm * 0.7, f32::from),
            underline_offset: under.map_or(upm * 0.1, |m| -f32::from(m.position)),
            underline_thickness: under.map_or(upm * 0.05, |m| f32::from(m.thickness)),
            strikeout_offset: strike.map_or(x_height * 0.5, |m| f32::from(m.position)),
            strikeout_thickness: strike.map_or(upm * 0.05, |m| f32::from(m.thickness)),
        };
        let mut points = Vec::new();
        if let Some(cmap) = face.tables().cmap {
            for sub in cmap.subtables.into_iter().filter(|s| s.is_unicode()) {
                sub.codepoints(|cp| {
                    if sub.glyph_index(cp).is_some_and(|g| g.0 != 0) {
                        points.push(cp);
                    }
                });
            }
        }
        points.sort_unstable();
        points.dedup();
        let mut coverage: Vec<(u32, u32)> = Vec::new();
        for cp in points {
            match coverage.last_mut() {
                Some(last) if last.1 + 1 == cp => last.1 = cp,
                _ => coverage.push((cp, cp)),
            }
        }
        Ok(Font { data, index, metrics, coverage: Arc::new(coverage) })
    }

    /// True when the font has a glyph for `c`. A table lookup: cheap enough to call per character.
    pub fn covers(&self, c: char) -> bool {
        let cp = c as u32;
        let i = self.coverage.partition_point(|r| r.1 < cp);
        self.coverage.get(i).is_some_and(|r| r.0 <= cp)
    }

    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    /// The font file, for embedding into a PDF or handing to another renderer.
    pub fn data(&self) -> &Arc<Vec<u8>> {
        &self.data
    }

    pub fn index(&self) -> u32 {
        self.index
    }

    /// Family name and style as the font file states them (typographic family preferred over the legacy one).
    pub fn describe(&self) -> Option<Description> {
        let face = ttf_parser::Face::parse(&self.data, self.index).ok()?;
        let name = |id: u16| {
            face.names().into_iter().filter(|n| n.name_id == id && n.is_unicode()).find_map(|n| n.to_string())
        };
        let family = name(ttf_parser::name_id::TYPOGRAPHIC_FAMILY).or_else(|| name(ttf_parser::name_id::FAMILY))?;
        Some(Description { family, bold: face.is_bold(), italic: face.is_italic() })
    }

    /// A shaper borrowing this font. Parsing the face is cheap but not free: make one per layout pass, not per word.
    pub fn shaper(&self) -> Result<Shaper<'_>, FontError> {
        let face = rustybuzz::Face::from_slice(&self.data, self.index).ok_or(FontError::Unreadable)?;
        Ok(Shaper { face, upm: self.metrics.units_per_em, plans: std::cell::RefCell::new(Vec::new()) })
    }
}

/// What a font file says about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Description {
    pub family: String,
    pub bold: bool,
    pub italic: bool,
}

/// One shaped glyph. Advances and offsets are in font units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub id: u16,
    /// Byte offset in the shaped string of the first character this glyph belongs to.
    pub cluster: u32,
    pub x_advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
}

/// Receives a glyph outline in font units, y up.
pub trait OutlineSink {
    fn move_to(&mut self, x: f32, y: f32);
    fn line_to(&mut self, x: f32, y: f32);
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32);
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32);
    fn close(&mut self);
}

struct SinkAdapter<'a>(&'a mut dyn OutlineSink);

impl ttf_parser::OutlineBuilder for SinkAdapter<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.curve_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.0.close();
    }
}

pub struct Shaper<'a> {
    face: rustybuzz::Face<'a>,
    upm: f32,
    /// Shaping plans by script and language. Making a plan (choosing the font's lookups) costs more than shaping
    /// a short word with it, and a report has two or three scripts.
    plans: std::cell::RefCell<Vec<(rustybuzz::Script, Option<rustybuzz::Language>, rustybuzz::ShapePlan)>>,
}

impl Shaper<'_> {
    /// Shape left-to-right text with the font's default features (kerning and standard ligatures, as browsers do).
    pub fn shape(&self, text: &str) -> Vec<Glyph> {
        if text.is_empty() {
            return Vec::new();
        }
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(text);
        buf.set_direction(rustybuzz::Direction::LeftToRight);
        buf.guess_segment_properties();
        let (script, language) = (buf.script(), buf.language());
        let mut plans = self.plans.borrow_mut();
        let known = plans.iter().position(|(s, l, _)| *s == script && *l == language);
        let index = known.unwrap_or_else(|| {
            let plan = rustybuzz::ShapePlan::new(
                &self.face,
                rustybuzz::Direction::LeftToRight,
                Some(script),
                language.as_ref(),
                &[],
            );
            plans.push((script, language.clone(), plan));
            plans.len() - 1
        });
        let Some((_, _, plan)) = plans.get(index) else { return Vec::new() };
        let out = rustybuzz::shape_with_plan(&self.face, plan, buf);
        out.glyph_infos()
            .iter()
            .zip(out.glyph_positions())
            .map(|(i, p)| Glyph {
                id: u16::try_from(i.glyph_id).unwrap_or(0),
                cluster: i.cluster,
                x_advance: p.x_advance as f32,
                x_offset: p.x_offset as f32,
                y_offset: p.y_offset as f32,
            })
            .collect()
    }

    /// Width of `text` in points at `size` points.
    pub fn advance(&self, text: &str, size: f32) -> f32 {
        self.shape(text).iter().map(|g| g.x_advance).sum::<f32>() * size / self.upm
    }

    /// Feed the outline of a glyph to `sink`; `false` when the glyph has none (a space).
    pub fn outline(&self, glyph: u16, sink: &mut dyn OutlineSink) -> bool {
        self.face.outline_glyph(GlyphId(glyph), &mut SinkAdapter(sink)).is_some()
    }
}

/// Index of a font inside a [`FontSet`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontId(pub u32);

/// What [`FontSet::resolve`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub id: FontId,
    /// The set has no bold (italic) face of the family: the caller may fake it or leave the text regular.
    pub missing_bold: bool,
    pub missing_italic: bool,
}

/// The fonts available to one engine instance, found by family name and style.
///
/// Word's font names resolve to their metric-compatible free counterparts, so a document that asks for
/// Times New Roman is laid out with Liberation Serif at the same line breaks.
#[derive(Default, Debug)]
pub struct FontSet {
    fonts: Vec<Font>,
    by_key: HashMap<(String, bool, bool), FontId>,
    default_family: Option<String>,
}

fn key(family: &str) -> String {
    family.trim().trim_matches(|c| c == '"' || c == '\'').to_lowercase()
}

/// Metric-compatible substitutes for the fonts Word documents ask for.
fn alias(family: &str) -> Option<&'static str> {
    Some(match family {
        "times new roman" | "times" | "serif" => "liberation serif",
        "arial" | "helvetica" | "sans-serif" => "liberation sans",
        "courier new" | "courier" | "monospace" => "liberation mono",
        "calibri" => "carlito",
        "cambria" => "caladea",
        _ => return None,
    })
}

impl FontSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a face. The first family added becomes the default for unknown names.
    pub fn add(&mut self, family: &str, bold: bool, italic: bool, font: Font) -> FontId {
        let id = FontId(self.fonts.len() as u32);
        self.fonts.push(font);
        let k = key(family);
        if self.default_family.is_none() {
            self.default_family = Some(k.clone());
        }
        self.by_key.insert((k, bold, italic), id);
        id
    }

    /// Register a face under the family and style its own file names. `None` when the file has no family name.
    pub fn add_described(&mut self, font: Font) -> Option<(FontId, Description)> {
        let d = font.describe()?;
        Some((self.add(&d.family, d.bold, d.italic, font), d))
    }

    pub fn get(&self, id: FontId) -> Option<&Font> {
        self.fonts.get(id.0 as usize)
    }

    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
    }

    pub fn len(&self) -> usize {
        self.fonts.len()
    }

    fn lookup(&self, family: &str, bold: bool, italic: bool) -> Option<Resolved> {
        // The exact style first, then drop italic, then bold, then both.
        for (b, i) in [(bold, italic), (bold, false), (false, italic), (false, false)] {
            if let Some(id) = self.by_key.get(&(family.to_owned(), b, i)) {
                return Some(Resolved { id: *id, missing_bold: bold && !b, missing_italic: italic && !i });
            }
        }
        None
    }

    /// A font for a character the preferred font lacks: the first registered face that has it, the wanted style
    /// first. `None` when no loaded font has the character (the caller draws the missing-glyph box).
    pub fn fallback_for(&self, c: char, bold: bool, italic: bool) -> Option<FontId> {
        let mut any = None;
        for ((_, b, i), id) in self.ordered_keys() {
            if self.get(id).is_some_and(|f| f.covers(c)) {
                if b == bold && i == italic {
                    return Some(id);
                }
                any = any.or(Some(id));
            }
        }
        any
    }

    /// Registered faces in registration order (a `HashMap` iteration would make layout nondeterministic).
    fn ordered_keys(&self) -> Vec<((String, bool, bool), FontId)> {
        let mut v: Vec<_> = self.by_key.iter().map(|(k, id)| (k.clone(), *id)).collect();
        v.sort_by_key(|(_, id)| id.0);
        v
    }

    /// The face for a family name and style: the family itself, its metric-compatible substitute, or the default
    /// family. `None` only when the set is empty.
    pub fn resolve(&self, family: Option<&str>, bold: bool, italic: bool) -> Option<Resolved> {
        if let Some(name) = family {
            // A CSS-like list: try every name in order.
            for part in name.split(',') {
                let k = key(part);
                if let Some(r) = self.lookup(&k, bold, italic) {
                    return Some(r);
                }
                if let Some(r) = alias(&k).and_then(|a| self.lookup(a, bold, italic)) {
                    return Some(r);
                }
            }
        }
        let default = self.default_family.clone()?;
        self.lookup(&default, bold, italic)
    }
}

#[cfg(test)]
mod tests;
