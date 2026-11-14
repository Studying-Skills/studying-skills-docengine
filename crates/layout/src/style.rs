//! Default formatting (DSTU 3008, the same numbers the DOCX export writes) and resolution of what a paragraph and a
//! run of text actually look like.

use sfdoc_fonts::{FontId, FontSet};
use sfdoc_geom::cm;
use sfdoc_model::{Align, Color, Marks, ParaProps, Paragraph, StyleDef, Styles, VertAlign};

/// Default tab stop and paragraph first-line indent: 1.25 cm.
pub const TAB: f32 = 35.433_07;
pub const BODY_FONT: &str = "Times New Roman";
pub const CODE_FONT: &str = "Courier New";
pub const BODY_SIZE: f32 = 14.0;
pub const CODE_SIZE: f32 = 12.0;
pub const LINK_COLOR: Color = Color { r: 0x11, g: 0x55, b: 0xcc };
/// Highlight of text under an open comment (the editor's translucent amber on white).
pub const COMMENT_COLOR: Color = Color { r: 0xff, g: 0xec, b: 0xb0 };
pub const CODE_BLOCK_COLOR: Color = Color { r: 0xf4, g: 0xf4, b: 0xf4 };
/// Superscript and subscript: size factor and baseline shift as a share of the base size.
pub const SCRIPT_SCALE: f32 = 0.65;
pub const SUP_RISE: f32 = 0.35;
pub const SUB_DROP: f32 = 0.15;
/// Left indent of every list level and the hanging indent of its marker (709 and 360 twips in the DOCX export).
pub const LIST_INDENT: f32 = 35.45;
pub const LIST_HANGING: f32 = 18.0;

/// Where a paragraph sits: decides the defaults it starts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Context {
    Body,
    ListItem,
    Quote,
    Cell { header: bool },
    Footnote,
}

/// Character formatting a paragraph's text starts from.
#[derive(Clone, Debug, PartialEq)]
pub struct TextDefaults {
    pub family: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
}

/// Fully resolved paragraph formatting, in points.
#[derive(Clone, Debug, PartialEq)]
pub struct ParaStyle {
    pub align: Align,
    pub line_mult: f32,
    pub left: f32,
    pub right: f32,
    pub first: f32,
    pub space_before: f32,
    pub space_after: f32,
    pub keep_next: bool,
    pub keep_lines: bool,
    pub page_break_before: bool,
    pub code: bool,
    pub text: TextDefaults,
}

impl ParaStyle {
    /// A paragraph without indents or spacing: what the engine sets its own lines in (contents rows, page
    /// numbers, labels).
    pub fn plain(align: Align, line_mult: f32, family: &str, size: f32, bold: bool) -> Self {
        Self {
            align,
            line_mult,
            left: 0.0,
            right: 0.0,
            first: 0.0,
            space_before: 0.0,
            space_after: 0.0,
            keep_next: false,
            keep_lines: false,
            page_break_before: false,
            code: false,
            text: TextDefaults { family: family.to_owned(), size, bold, italic: false },
        }
    }
}

/// The body font of a document: its `body` style's, or the engine's default.
pub fn body_font(styles: &Styles) -> &str {
    styles.get("body").and_then(|s| s.font.as_deref()).unwrap_or(BODY_FONT)
}

/// Which parts of a style definition a paragraph takes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Take {
    /// The font family only.
    Font,
    /// The font and its size.
    FontSize,
    /// Everything but the first-line indent.
    NoIndent,
    All,
}

fn apply_def(s: &mut ParaStyle, d: &StyleDef, take: Take) {
    if let Some(f) = &d.font {
        s.text.family.clone_from(f);
    }
    if take == Take::Font {
        return;
    }
    if let Some(v) = d.size {
        s.text.size = v;
    }
    if take == Take::FontSize {
        return;
    }
    if let Some(v) = d.bold {
        s.text.bold = v;
    }
    if let Some(v) = d.italic {
        s.text.italic = v;
    }
    if let Some(v) = d.align {
        s.align = v;
    }
    if let Some(v) = d.line_height {
        s.line_mult = v;
    }
    if let Some(v) = d.space_before {
        s.space_before = v.max(0.0);
    }
    if let Some(v) = d.space_after {
        s.space_after = v.max(0.0);
    }
    if take == Take::All
        && let Some(v) = d.indent_first
    {
        s.first = v;
    }
}

/// The formatting of `p` in `ctx`: defaults for its kind, changed by the document's styles, overridden by what
/// the paragraph states itself.
pub fn para_style(p: &Paragraph, ctx: Context, styles: &Styles) -> ParaStyle {
    let body = TextDefaults { family: BODY_FONT.to_owned(), size: BODY_SIZE, bold: false, italic: false };
    let mut s = ParaStyle {
        align: Align::Justify,
        line_mult: 1.5,
        left: 0.0,
        right: 0.0,
        first: 0.0,
        space_before: 0.0,
        space_after: 0.0,
        keep_next: false,
        keep_lines: false,
        page_break_before: false,
        code: false,
        text: body,
    };
    match ctx {
        Context::Body => s.first = cm(1.25),
        Context::ListItem => {}
        Context::Quote => s.text.italic = true,
        Context::Cell { header } => {
            s.align = Align::Left;
            s.line_mult = 1.0;
            s.text.bold = header;
        }
        Context::Footnote => {
            s.align = Align::Left;
            s.line_mult = 1.0;
            s.text.size = 10.0;
        }
    }
    // The body style: its font is the document's font everywhere; the rest is for body text where it flows.
    if let Some(d) = styles.get("body") {
        let take = match ctx {
            _ if p.level > 0 => Take::Font,
            Context::Body => Take::All,
            Context::ListItem | Context::Quote => Take::NoIndent,
            Context::Cell { .. } => Take::FontSize,
            Context::Footnote => Take::Font,
        };
        apply_def(&mut s, d, take);
    }
    match p.level {
        0 => {}
        level => {
            // Headings: never indented, kept with the paragraph that follows.
            s.first = 0.0;
            s.keep_next = true;
            s.keep_lines = true;
            let (size, bold, italic, before, after, align) = match level {
                1 => (16.0, true, false, 12.0, 6.0, Align::Left),
                2 => (14.0, true, false, 12.0, 6.0, Align::Left),
                3 => (14.0, true, true, 6.0, 3.0, Align::Left),
                _ => (14.0, false, true, 0.0, 0.0, Align::Justify),
            };
            s.text = TextDefaults { family: std::mem::take(&mut s.text.family), size, bold, italic };
            s.space_before = before;
            s.space_after = after;
            s.align = align;
            if let Some(d) = styles.get(&format!("h{level}")) {
                apply_def(&mut s, d, Take::All);
            }
        }
    }
    if p.props.code {
        s.code = true;
        s.first = 0.0;
        s.align = Align::Left;
        s.line_mult = 1.0;
        s.text = TextDefaults { family: CODE_FONT.to_owned(), size: CODE_SIZE, bold: false, italic: false };
        if let Some(d) = styles.get("code") {
            apply_def(&mut s, d, Take::All);
        }
    } else if p.level == 0
        && let Some(d) = p.props.style.as_deref().and_then(|name| styles.get(name))
    {
        // A style the paragraph names itself (a caption).
        apply_def(&mut s, d, Take::All);
    }
    apply_props(&mut s, &p.props);
    s
}

fn apply_props(s: &mut ParaStyle, p: &ParaProps) {
    if let Some(a) = p.align {
        s.align = a;
    }
    if let Some(v) = p.line_height {
        s.line_mult = v;
    }
    if let Some(v) = p.indent_left {
        s.left = v.max(0.0);
    }
    if let Some(v) = p.indent_right {
        s.right = v.max(0.0);
    }
    if let Some(v) = p.indent_first {
        s.first = v;
    }
    if let Some(v) = p.space_before {
        s.space_before = v.max(0.0);
    }
    if let Some(v) = p.space_after {
        s.space_after = v.max(0.0);
    }
    s.keep_next |= p.keep_next;
    s.keep_lines |= p.keep_lines;
    s.page_break_before |= p.page_break_before;
}

/// Fully resolved character formatting of a run.
#[derive(Clone, Debug, PartialEq)]
pub struct RunStyle {
    pub font: FontId,
    pub bold: bool,
    pub italic: bool,
    /// Size the glyphs are drawn at (already reduced for super/subscript).
    pub size: f32,
    /// Baseline shift upwards in points (negative = down).
    pub rise: f32,
    pub color: Color,
    pub background: Option<Color>,
    pub underline: bool,
    pub strike: bool,
    pub link: Option<String>,
}

/// The look of text with `marks` inside a paragraph whose text starts from `base`.
/// `None` only when no fonts are loaded.
pub fn run_style(marks: &Marks, base: &TextDefaults, fonts: &FontSet) -> Option<RunStyle> {
    let family = marks.font.as_deref().unwrap_or(if marks.code { CODE_FONT } else { &base.family });
    let bold = base.bold || marks.bold;
    let italic = base.italic || marks.italic;
    let full =
        marks.size.unwrap_or(if marks.code && marks.font.is_none() { CODE_SIZE.min(base.size) } else { base.size });
    let (size, rise) = match marks.vert {
        VertAlign::Baseline => (full, 0.0),
        VertAlign::Sup => (full * SCRIPT_SCALE, full * SUP_RISE),
        VertAlign::Sub => (full * SCRIPT_SCALE, -full * SUB_DROP),
    };
    let link = marks.link.clone();
    Some(RunStyle {
        font: fonts.resolve(Some(family), bold, italic)?.id,
        bold,
        italic,
        size,
        rise,
        color: marks.color.unwrap_or(if link.is_some() { LINK_COLOR } else { Color::BLACK }),
        background: marks.background.or(if marks.comments.is_empty() { None } else { Some(COMMENT_COLOR) }),
        underline: marks.underline || link.is_some(),
        strike: marks.strike,
        link,
    })
}
