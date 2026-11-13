//! Paragraph layout: inline content → lines of positioned fragments.
//!
//! Text is cut at line-break opportunities (UAX #14) and at changes of formatting or font; every piece is shaped on
//! its own. Painters place each piece separately, so widths agree with the browser to a fraction of a pixel (a line
//! shaped as one string does not: browsers kern around spaces differently). Lines are filled greedily, as Word does.

use std::collections::VecDeque;

use sfdoc_fonts::{FontId, FontSet, Glyph, Shaper};
use sfdoc_model::{Align, AtomKind, Color, Inline, Paragraph, utf16_len};

use crate::display::PlacedGlyph;
use crate::style::{ParaStyle, RunStyle, SCRIPT_SCALE, SUP_RISE, TAB, run_style};

/// One shaper per loaded font, made once per layout pass.
pub struct Shapers<'a> {
    fonts: &'a FontSet,
    shapers: Vec<Option<Shaper<'a>>>,
}

impl<'a> Shapers<'a> {
    pub fn new(fonts: &'a FontSet) -> Self {
        let shapers = (0..fonts.len() as u32).map(|i| fonts.get(FontId(i)).and_then(|f| f.shaper().ok())).collect();
        Self { fonts, shapers }
    }
    pub fn fonts(&self) -> &'a FontSet {
        self.fonts
    }
    fn get(&self, id: FontId) -> Option<&Shaper<'a>> {
        self.shapers.get(id.0 as usize).and_then(Option::as_ref)
    }
}

/// A drawn piece of a line. Offsets `from..to` are UTF-16 units from the start of the paragraph's content.
#[derive(Clone, Debug, PartialEq)]
pub struct Frag {
    /// Left edge, from the left edge of the paragraph's box.
    pub x: f32,
    /// Distance to the next fragment's left edge, trailing spaces included.
    pub advance: f32,
    pub from: u32,
    pub to: u32,
    /// Caret offset from `x` for every position `from..=to`.
    pub carets: Vec<f32>,
    /// Extent above and below the line's baseline.
    pub ascent: f32,
    pub descent: f32,
    pub kind: FragKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FragKind {
    Text(TextFrag),
    /// An object sized outside the engine: its box sits on the baseline, `depth` below it.
    Atom {
        kind: AtomKind,
        width: f32,
        height: f32,
        depth: f32,
        data: Option<String>,
    },
    /// Takes positions and width but draws nothing: a tab or the end of a line.
    Space,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextFrag {
    pub font: FontId,
    pub size: f32,
    pub rise: f32,
    pub color: Color,
    pub background: Option<Color>,
    pub underline: bool,
    pub strike: bool,
    pub link: Option<String>,
    pub glyphs: Vec<PlacedGlyph>,
    pub text: String,
    /// Width of the visible part (trailing spaces excluded).
    pub ink_width: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Top of the line box from the top of the paragraph.
    pub top: f32,
    pub height: f32,
    /// Baseline from the top of the line box.
    pub baseline: f32,
    /// Extent of the content below the baseline (the caret spans `baseline + descent`).
    pub descent: f32,
    pub frags: Vec<Frag>,
    pub from: u32,
    pub to: u32,
    /// Left edge of the text and its width (trailing spaces excluded), from the paragraph box's left edge.
    pub x: f32,
    pub width: f32,
    /// The line ends its paragraph or ends with a forced break (such lines are never justified).
    pub hard: bool,
    /// Hash of what the line looks like (not of where it is or which positions it holds): page fingerprints are
    /// made of these, so an unchanged line is hashed once, not on every keystroke.
    pub look: u64,
}

/// See [`Line::look`].
fn look_of(height: f32, baseline: f32, descent: f32, frags: &[Frag]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for v in [height, baseline, descent] {
        v.to_bits().hash(&mut h);
    }
    frags.len().hash(&mut h);
    for f in frags {
        f.x.to_bits().hash(&mut h);
        f.advance.to_bits().hash(&mut h);
        match &f.kind {
            FragKind::Text(t) => {
                0u8.hash(&mut h);
                t.text.hash(&mut h);
                t.font.0.hash(&mut h);
                t.size.to_bits().hash(&mut h);
                t.rise.to_bits().hash(&mut h);
                t.color.hash(&mut h);
                t.background.hash(&mut h);
                (t.underline, t.strike).hash(&mut h);
                t.link.hash(&mut h);
            }
            FragKind::Atom { kind, width, height, depth, data } => {
                1u8.hash(&mut h);
                (*kind as u8).hash(&mut h);
                for v in [width, height, depth] {
                    v.to_bits().hash(&mut h);
                }
                data.hash(&mut h);
            }
            FragKind::Space => 2u8.hash(&mut h),
        }
    }
    h.finish()
}

pub struct ParaInput<'a> {
    pub para: &'a Paragraph,
    pub style: &'a ParaStyle,
    /// Width of the paragraph's box.
    pub width: f32,
    /// List marker hanging to the left of the first line, and its left edge.
    pub marker: Option<(&'a str, f32)>,
    /// Number of footnotes before this paragraph: its first footnote mark is `footnote_base + 1`.
    pub footnote_base: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Brk {
    No,
    Allowed,
    Mandatory,
}

enum UnitKind {
    Text { style: usize, font: FontId, glyphs: Vec<PlacedGlyph>, text: String },
    Tab,
    Newline,
    Atom(usize),
}

/// How much of their width the spaces of a justified line may give up so that one more word fits.
const SHRINK: f32 = 0.25;

/// What the trailing spaces of a unit may give up. Counted per space in whole twentieths of a point, rounded
/// down - that is how LibreOffice does it when it follows Word's rule, and with it 99 % of 975 justified lines
/// of the comparison corpus break where LibreOffice breaks them.
fn squeeze_of(u: &Unit) -> f32 {
    if u.trail_spaces == 0 {
        return 0.0;
    }
    let one = u.trail / u.trail_spaces as f32;
    (one * 20.0 * SHRINK).floor() / 20.0 * u.trail_spaces as f32
}

/// An unbreakable piece of the paragraph before it is put on a line.
struct Unit {
    kind: UnitKind,
    from: u32,
    len: u32,
    width: f32,
    /// Width and count of trailing spaces (they hang past the right edge and stretch when justifying).
    trail: f32,
    trail_spaces: u32,
    carets: Vec<f32>,
    ascent: f32,
    descent: f32,
    line_h: f32,
    brk: Brk,
}

fn is_joiner(c: char) -> bool {
    matches!(c, '\u{300}'..='\u{36F}' | '\u{FE00}'..='\u{FE0F}' | '\u{200D}' | '\u{200C}')
}

/// Caret x for every UTF-16 boundary of `text`, from the glyphs' clusters. Characters sharing a cluster (a ligature,
/// a letter with a combining mark) divide its width evenly.
fn carets(text: &str, glyphs: &[Glyph], k: f32) -> (Vec<f32>, Vec<PlacedGlyph>, f32) {
    let mut placed = Vec::with_capacity(glyphs.len());
    let mut clusters: Vec<(usize, f32)> = Vec::new();
    let mut pen = 0.0f32;
    for g in glyphs {
        if clusters.last().is_none_or(|c| c.0 != g.cluster as usize) {
            clusters.push((g.cluster as usize, pen));
        }
        placed.push(PlacedGlyph { id: g.id, cluster: g.cluster, x: pen + g.x_offset * k, y: -g.y_offset * k });
        pen += g.x_advance * k;
    }
    clusters.sort_by_key(|c| c.0);
    clusters.push((text.len(), pen));
    let mut out = Vec::with_capacity(text.len() + 1);
    let mut ci = 0;
    for (b, c) in text.char_indices() {
        while clusters.get(ci + 1).is_some_and(|n| n.0 <= b) {
            ci += 1;
        }
        let (b0, x0) = clusters.get(ci).copied().unwrap_or((0, 0.0));
        let (b1, x1) = clusters.get(ci + 1).copied().unwrap_or((text.len(), pen));
        let inside = text.get(b0..b1).unwrap_or("");
        let n = inside.chars().count().max(1) as f32;
        let j = text.get(b0..b).map_or(0, |s| s.chars().count()) as f32;
        let x = x0 + (x1 - x0) * j / n;
        for _ in 0..c.len_utf16() {
            out.push(x);
        }
    }
    out.push(pen);
    (out, placed, pen)
}

struct Builder<'a, 'f> {
    sh: &'a Shapers<'f>,
    styles: Vec<RunStyle>,
    atoms: Vec<&'a sfdoc_model::Atom>,
}

impl Builder<'_, '_> {
    fn text_unit(&self, text: &str, style: usize, font: FontId, from: u32, brk: Brk) -> Option<Unit> {
        let st = self.styles.get(style)?;
        let f = self.sh.fonts.get(font)?;
        let m = f.metrics();
        let k = st.size / m.units_per_em;
        let glyphs = self.sh.get(font)?.shape(text);
        let (carets, placed, width) = carets(text, &glyphs, k);
        let trimmed = text.trim_end_matches(' ');
        let trail_spaces = (text.len() - trimmed.len()) as u32;
        let trail = width - carets.get(utf16_len(trimmed) as usize).copied().unwrap_or(width);
        Some(Unit {
            kind: UnitKind::Text { style, font, glyphs: placed, text: text.to_owned() },
            from,
            len: utf16_len(text),
            width,
            trail,
            trail_spaces,
            carets,
            ascent: m.ascent * k + st.rise,
            descent: m.descent * k - st.rise,
            line_h: m.line_height(st.size),
            brk,
        })
    }

    /// Cut a text slice wherever the font that has the next character changes, and shape the parts.
    fn text_units(&self, text: &str, style: usize, mut from: u32, brk: Brk, out: &mut VecDeque<Unit>) -> Option<()> {
        let st = self.styles.get(style)?;
        let fonts = self.sh.fonts;
        let primary = fonts.get(st.font)?;
        let pick = |c: char| {
            if c.is_whitespace() || c.is_control() || primary.covers(c) {
                st.font
            } else {
                fonts.fallback_for(c, st.bold, st.italic).unwrap_or(st.font)
            }
        };
        let mut start = 0;
        let mut current: Option<FontId> = None;
        for (b, c) in text.char_indices() {
            let font = if is_joiner(c) { current.unwrap_or(st.font) } else { pick(c) };
            if let Some(cur) = current
                && cur != font
            {
                let part = text.get(start..b)?;
                out.push_back(self.text_unit(part, style, cur, from, Brk::No)?);
                from += utf16_len(part);
                start = b;
            }
            current = Some(font);
        }
        let part = text.get(start..)?;
        if !part.is_empty() {
            out.push_back(self.text_unit(part, style, current.unwrap_or(st.font), from, brk)?);
        }
        Some(())
    }

    fn fixed_unit(&self, kind: UnitKind, from: u32, width: f32, ascent: f32, descent: f32, brk: Brk) -> Unit {
        Unit {
            kind,
            from,
            len: 1,
            width,
            trail: 0.0,
            trail_spaces: 0,
            carets: vec![0.0, width],
            ascent,
            descent,
            line_h: 0.0,
            brk,
        }
    }
}

/// Metrics of the paragraph's own font: an empty line is as tall as a line of text.
fn base_metrics(style: &ParaStyle, fonts: &FontSet) -> Option<(f32, f32, f32)> {
    let id = fonts.resolve(Some(style.text.family), style.text.bold, style.text.italic)?.id;
    let m = fonts.get(id)?.metrics();
    let k = style.text.size / m.units_per_em;
    Some((m.ascent * k, m.descent * k, m.line_height(style.text.size)))
}

/// Lay a paragraph out into lines. `None` only when no fonts are loaded.
/// An object wider than the paragraph is scaled down to its width: it would otherwise stick out of the page or of
/// its table cell. Returns the factor.
fn atom_scale(a: &sfdoc_model::Atom, inp: &ParaInput<'_>) -> f32 {
    let room = (inp.width - inp.style.left - inp.style.right - inp.style.first.max(0.0)).max(1.0);
    if a.width > room { room / a.width } else { 1.0 }
}

pub fn layout_paragraph(inp: &ParaInput<'_>, sh: &Shapers<'_>) -> Option<Vec<Line>> {
    layout_impl(inp, sh, None)
}

/// Widths the paragraph's text asks for, indents included: all on one line, and with every allowed break taken
/// (the widest piece that cannot be broken). Table columns are sized from these.
pub fn paragraph_widths(para: &Paragraph, style: &ParaStyle, sh: &Shapers<'_>) -> Option<(f32, f32)> {
    let inp = ParaInput { para, style, width: 1.0e6, marker: None, footnote_base: 0 };
    let mut min = 0.0f32;
    let lines = layout_impl(&inp, sh, Some(&mut min))?;
    let indent = style.left + style.right + style.first.max(0.0);
    // Forced breaks make several lines even in unlimited width: the widest one counts.
    Some((indent + lines.iter().map(|l| l.width).fold(0.0, f32::max), indent + min))
}

fn layout_impl(inp: &ParaInput<'_>, sh: &Shapers<'_>, min_content: Option<&mut f32>) -> Option<Vec<Line>> {
    let fonts = sh.fonts;
    let style = inp.style;
    let base = base_metrics(style, fonts)?;
    let mut b = Builder { sh, styles: Vec::new(), atoms: Vec::new() };

    // 1. The analysis string: the paragraph's text with one stand-in character per atom, so that its UTF-16 offsets
    //    are the paragraph's content offsets.
    enum Span {
        Text { style: usize },
        Atom { index: usize },
    }
    let mut s = String::new();
    let mut spans: Vec<(usize, usize, Span)> = Vec::new();
    let mut footnotes = inp.footnote_base;
    let mut footnote_numbers = Vec::new();
    for inline in &inp.para.inlines {
        let start = s.len();
        match inline {
            Inline::Text(t) => {
                if t.text.is_empty() {
                    continue;
                }
                b.styles.push(run_style(&t.marks, &style.text, fonts)?);
                s.push_str(&t.text);
                spans.push((start, s.len(), Span::Text { style: b.styles.len() - 1 }));
            }
            Inline::Atom(a) => {
                // A forced break is a newline for the break algorithm; any other atom behaves like a letter, so it
                // is not torn from the word or the punctuation it touches.
                s.push(if a.kind == AtomKind::Break { '\n' } else { 'A' });
                if a.kind == AtomKind::Footnote {
                    footnotes += 1;
                    footnote_numbers.push((b.atoms.len(), footnotes));
                }
                b.atoms.push(a);
                spans.push((start, s.len(), Span::Atom { index: b.atoms.len() - 1 }));
            }
        }
    }

    // 2. Break opportunities by byte offset.
    let mut opps: Vec<(usize, bool)> = unicode_linebreak::linebreaks(&s)
        .map(|(i, o)| (i, o == unicode_linebreak::BreakOpportunity::Mandatory))
        .collect();
    opps.sort_unstable();
    let brk_at = |i: usize| match opps.binary_search_by_key(&i, |o| o.0) {
        Ok(k) => match opps.get(k) {
            Some((_, true)) => Brk::Mandatory,
            _ => Brk::Allowed,
        },
        Err(_) => Brk::No,
    };

    // 3. Units.
    let mut units: VecDeque<Unit> = VecDeque::new();
    let mut pos16 = 0u32;
    // Superscript style for footnote marks, made on first use.
    let mut footnote_style: Option<usize> = None;
    for (start, end, span) in &spans {
        match span {
            Span::Atom { index } => {
                let Some(a) = b.atoms.get(*index).copied() else { continue };
                let brk = brk_at(*end);
                let unit = match a.kind {
                    AtomKind::Break => b.fixed_unit(UnitKind::Newline, pos16, 0.0, 0.0, 0.0, Brk::Mandatory),
                    AtomKind::Footnote => {
                        let st = match footnote_style {
                            Some(i) => i,
                            None => {
                                let mut st = run_style(&sfdoc_model::Marks::default(), &style.text, fonts)?;
                                st.rise = st.size * SUP_RISE;
                                st.size *= SCRIPT_SCALE;
                                b.styles.push(st);
                                footnote_style = Some(b.styles.len() - 1);
                                b.styles.len() - 1
                            }
                        };
                        let n = footnote_numbers.iter().find(|f| f.0 == *index).map_or(0, |f| f.1);
                        let font = b.styles.get(st)?.font;
                        let mut u = b.text_unit(&n.to_string(), st, font, pos16, brk)?;
                        // The mark is one position however many digits it shows.
                        u.len = 1;
                        u.carets = vec![0.0, u.width];
                        u
                    }
                    _ => {
                        let k = atom_scale(a, inp);
                        b.fixed_unit(
                            UnitKind::Atom(*index),
                            pos16,
                            a.width * k,
                            (a.height - a.depth).max(0.0) * k,
                            a.depth.max(0.0) * k,
                            brk,
                        )
                    }
                };
                units.push_back(unit);
                pos16 += 1;
            }
            Span::Text { style: st } => {
                let text = s.get(*start..*end)?;
                let mut piece = 0usize;
                let flush = |from_b: usize, to_b: usize, pos16: &mut u32, units: &mut VecDeque<Unit>| -> Option<()> {
                    if to_b > from_b {
                        let part = text.get(from_b..to_b)?;
                        b.text_units(part, *st, *pos16, brk_at(start + to_b), units)?;
                        *pos16 += utf16_len(part);
                    }
                    Some(())
                };
                for (i, c) in text.char_indices() {
                    let abs = start + i;
                    if c == '\t' || c == '\n' {
                        flush(piece, i, &mut pos16, &mut units)?;
                        let kind = if c == '\t' { UnitKind::Tab } else { UnitKind::Newline };
                        let brk = if c == '\n' { Brk::Mandatory } else { brk_at(abs + 1) };
                        units.push_back(b.fixed_unit(kind, pos16, 0.0, 0.0, 0.0, brk));
                        pos16 += 1;
                        piece = i + 1;
                    } else if i > piece && brk_at(abs) != Brk::No {
                        flush(piece, i, &mut pos16, &mut units)?;
                        piece = i;
                    }
                }
                flush(piece, text.len(), &mut pos16, &mut units)?;
            }
        }
    }

    if let Some(min) = min_content {
        // Units glued to the next one (no break allowed between them) count together.
        let mut run = 0.0f32;
        for u in &units {
            run += u.width;
            if u.brk != Brk::No {
                *min = min.max(run - u.trail);
                run = 0.0;
            }
        }
        *min = min.max(run);
    }

    // 4. Fill lines greedily.
    let mut lines: Vec<Line> = Vec::new();
    let mut y = 0.0f32;
    let text_left = |first: bool| style.left + if first { style.first } else { 0.0 };
    let avail = |first: bool| (inp.width - text_left(first) - style.right).max(1.0);
    let mut current: Vec<Unit> = Vec::new();
    let mut x = 0.0f32;
    let mut content_end = 0u32;

    let finish = |current: &mut Vec<Unit>, hard: bool, lines: &mut Vec<Line>, y: &mut f32, line_from: u32| {
        let first = lines.is_empty();
        let left = text_left(first);
        let room = avail(first);
        let last_trail = current.last().map_or(0.0, |u| u.trail);
        let natural: f32 = current.iter().map(|u| u.width).sum::<f32>() - last_trail;
        // Spaces that may stretch: trailing spaces of every unit but the last one on the line.
        let gaps: u32 = current.iter().rev().skip(1).map(|u| u.trail_spaces).sum();
        let free = (room - natural).max(0.0);
        let (offset, extra) = match style.align {
            Align::Left => (0.0, 0.0),
            Align::Center => (free / 2.0, 0.0),
            Align::Right => (free, 0.0),
            // A line that took one word more than fits (see `SHRINK`): its spaces narrow, the last line's too.
            Align::Justify if gaps > 0 && natural > room => (0.0, (room - natural) / gaps as f32),
            Align::Justify if !hard && gaps > 0 => (0.0, free / gaps as f32),
            Align::Justify => (0.0, 0.0),
        };
        let (mut ascent, mut descent, mut natural_h) =
            if current.iter().any(|u| matches!(u.kind, UnitKind::Text { .. })) {
                (0.0f32, 0.0f32, 0.0f32)
            } else {
                base
            };
        let mut frags = Vec::with_capacity(current.len() + 1);
        if first && let Some((text, mx)) = inp.marker {
            // The marker takes the paragraph's base font and no positions.
            if let Some(st) = run_style(&sfdoc_model::Marks::default(), &style.text, fonts)
                && let Some(f) = fonts.get(st.font)
                && let Some(shaper) = sh.get(st.font)
            {
                let m = f.metrics();
                let k = st.size / m.units_per_em;
                let (_, placed, width) = carets(text, &shaper.shape(text), k);
                ascent = ascent.max(m.ascent * k);
                descent = descent.max(m.descent * k);
                natural_h = natural_h.max(m.line_height(st.size));
                frags.push(Frag {
                    x: mx,
                    advance: width,
                    from: line_from,
                    to: line_from,
                    carets: vec![0.0],
                    ascent: m.ascent * k,
                    descent: m.descent * k,
                    kind: FragKind::Text(TextFrag {
                        font: st.font,
                        size: st.size,
                        rise: 0.0,
                        color: st.color,
                        background: None,
                        underline: false,
                        strike: false,
                        link: None,
                        glyphs: placed,
                        text: text.to_owned(),
                        ink_width: width,
                    }),
                });
            }
        }
        let mut pen = left + offset;
        let count = current.len();
        let mut to = line_from;
        for (i, u) in current.drain(..).enumerate() {
            let stretch = if i + 1 < count { extra } else { 0.0 };
            let mut carets = u.carets;
            if stretch != 0.0 && u.trail_spaces > 0 {
                // Each trailing space grows (or narrows) by `stretch`: carets after the k-th space move k times.
                let n = carets.len();
                for (k, c) in carets.iter_mut().skip(n.saturating_sub(u.trail_spaces as usize)).enumerate() {
                    *c += stretch * (k + 1) as f32;
                }
            }
            let advance = u.width + stretch * u.trail_spaces as f32;
            ascent = ascent.max(u.ascent);
            descent = descent.max(u.descent);
            natural_h = natural_h.max(u.line_h);
            to = u.from + u.len;
            let kind = match u.kind {
                UnitKind::Text { style: st, font, glyphs, text } => match b.styles.get(st) {
                    Some(s) => FragKind::Text(TextFrag {
                        font,
                        size: s.size,
                        rise: s.rise,
                        color: s.color,
                        background: s.background,
                        underline: s.underline,
                        strike: s.strike,
                        link: s.link.clone(),
                        glyphs,
                        text,
                        ink_width: u.width - u.trail,
                    }),
                    None => FragKind::Space,
                },
                UnitKind::Atom(index) => match b.atoms.get(index) {
                    Some(a) => {
                        let k = atom_scale(a, inp);
                        FragKind::Atom {
                            kind: a.kind,
                            width: a.width * k,
                            height: a.height * k,
                            depth: a.depth * k,
                            data: a.data.clone(),
                        }
                    }
                    None => FragKind::Space,
                },
                UnitKind::Tab | UnitKind::Newline => FragKind::Space,
            };
            frags.push(Frag { x: pen, advance, from: u.from, to, carets, ascent: u.ascent, descent: u.descent, kind });
            pen += advance;
        }
        // Word's "multiple" spacing: a multiple of the tallest font's single line height; the extra goes below.
        let height = (natural_h * style.line_mult).max(ascent + descent);
        // The font's own line gap stays above the text, where Word and LibreOffice keep it.
        let ascent = ascent + (natural_h - ascent - descent).max(0.0);
        lines.push(Line {
            top: *y,
            height,
            baseline: ascent,
            descent,
            look: look_of(height, ascent, descent, &frags),
            frags,
            from: line_from,
            to,
            x: left + offset,
            width: natural,
            hard,
        });
        *y += height;
    };

    let mut line_from = 0u32;
    while let Some(first_unit) = units.pop_front() {
        // A segment: units up to and including the next break opportunity.
        let mut seg = vec![first_unit];
        while seg.last().is_some_and(|u| u.brk == Brk::No) {
            match units.pop_front() {
                Some(u) => seg.push(u),
                None => break,
            }
        }
        let first = lines.is_empty();
        let room = avail(first);
        // Tabs get their width from where they land on the line.
        let mut seg_x = x;
        for u in &mut seg {
            if matches!(u.kind, UnitKind::Tab) {
                let abs = text_left(first) + seg_x;
                u.width = ((abs / TAB).floor() + 1.0) * TAB - abs;
                u.carets = vec![0.0, u.width];
            }
            seg_x += u.width;
        }
        let seg_trim = seg_x - seg.last().map_or(0.0, |u| u.trail);
        const EPS: f32 = 0.01;
        // Justified lines may narrow their spaces to take one more word, as Word (2013 and later) does.
        let squeeze = if style.align == Align::Justify && !current.is_empty() {
            current.iter().chain(seg.iter().rev().skip(1)).map(squeeze_of).sum::<f32>()
        } else {
            0.0
        };
        if seg_trim > room + EPS + squeeze && !current.is_empty() {
            // Does not fit after what is already on the line: wrap and try again on a fresh line.
            for u in seg.into_iter().rev() {
                units.push_front(u);
            }
            finish(&mut current, false, &mut lines, &mut y, line_from);
            line_from = content_end;
            x = 0.0;
            continue;
        }
        if seg_trim > room + EPS && current.is_empty() {
            // Wider than a whole line: put what fits, break inside a word if it comes to that.
            let mut rest: VecDeque<Unit> = seg.into();
            let mut placed_any = false;
            while let Some(u) = rest.pop_front() {
                if x + u.width - u.trail <= room + EPS {
                    x += u.width;
                    content_end = u.from + u.len;
                    current.push(u);
                    placed_any = true;
                    continue;
                }
                let split = match &u.kind {
                    UnitKind::Text { style: st, font, text, .. } => {
                        // The last character boundary whose caret still fits; at least one character on an empty line.
                        let mut cut = None;
                        let mut u16 = 0usize;
                        for (bi, c) in text.char_indices() {
                            let next16 = u16 + c.len_utf16();
                            let next_b = bi + c.len_utf8();
                            let joins = text.get(next_b..).and_then(|r| r.chars().next()).is_some_and(is_joiner);
                            if !joins && next_b < text.len() {
                                let cx = u.carets.get(next16).copied().unwrap_or(f32::MAX);
                                if x + cx <= room + EPS || (cut.is_none() && !placed_any) {
                                    cut = Some(next_b);
                                }
                                if x + cx > room + EPS {
                                    break;
                                }
                            }
                            u16 = next16;
                        }
                        cut.and_then(|cb| {
                            let (head, tail) = (text.get(..cb)?, text.get(cb..)?);
                            Some((
                                b.text_unit(head, *st, *font, u.from, Brk::Allowed)?,
                                b.text_unit(tail, *st, *font, u.from + utf16_len(head), u.brk)?,
                            ))
                        })
                    }
                    _ => None,
                };
                match split {
                    Some((head, tail)) => {
                        content_end = head.from + head.len;
                        current.push(head);
                        rest.push_front(tail);
                    }
                    // A single character or object wider than the line: it overflows rather than loop forever.
                    None if !placed_any => {
                        content_end = u.from + u.len;
                        current.push(u);
                    }
                    None => rest.push_front(u),
                }
                break;
            }
            let hard = rest.is_empty() && current.last().is_some_and(|u| u.brk == Brk::Mandatory);
            for u in rest.into_iter().rev() {
                units.push_front(u);
            }
            finish(&mut current, hard, &mut lines, &mut y, line_from);
            line_from = content_end;
            x = 0.0;
            continue;
        }
        let mandatory = seg.last().is_some_and(|u| u.brk == Brk::Mandatory);
        x = seg_x;
        if let Some(last) = seg.last() {
            content_end = last.from + last.len;
        }
        current.extend(seg);
        if mandatory {
            let ended_by_newline = current.last().is_some_and(|u| matches!(u.kind, UnitKind::Newline));
            finish(&mut current, true, &mut lines, &mut y, line_from);
            line_from = content_end;
            x = 0.0;
            // A forced break at the very end leaves an empty last line for the caret to stand on.
            if ended_by_newline && units.is_empty() {
                finish(&mut current, true, &mut lines, &mut y, line_from);
            }
        }
    }
    if !current.is_empty() || lines.is_empty() {
        finish(&mut current, true, &mut lines, &mut y, line_from);
    }
    Some(lines)
}
