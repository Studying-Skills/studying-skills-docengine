//! From blocks to pages.
//!
//! Step 1 (`flatten`) turns every top-level block into a short list of flow items — paragraphs already broken into
//! lines, and boxes — with positions relative to the block. The result depends only on the block itself (plus the
//! page width and the number of footnotes before it), so it is cached and reused until the block changes.
//!
//! Step 2 (`paginate`) stacks the flow items of the whole document onto pages. It is plain arithmetic over line
//! heights and is redone from scratch after every edit, which is what makes the incremental layout equal to the
//! full one by construction.

use std::sync::Arc;

use sfdoc_geom::{Rect, cm};
use sfdoc_model::{
    Align, Block, Borders, Color, Leaf, LeafKind, List, PageNumber, PageSetup, Paragraph, Running, Styles, Table,
    VAlign,
};

use crate::para::{Line, Narrow, ParaInput, Shapers, layout_paragraph};
use crate::style::{BODY_SIZE, CODE_BLOCK_COLOR, Context, LIST_HANGING, LIST_INDENT, ParaStyle, body_font, para_style};

/// What the layout of a block depends on besides the block itself.
#[derive(Clone, Copy)]
pub struct Env<'a> {
    pub headings: &'a [Heading],
    pub styles: &'a Styles,
    pub hyphenate: bool,
    /// Height of the page's text area.
    pub page_height: f32,
}

/// Between a picture the text flows around and the text beside it.
const FLOAT_GAP: f32 = 9.0;
/// A picture wider than this share of the text area, or taller than this share of the page, stands alone.
const FLOAT_MAX_WIDTH: f32 = 0.6;
const FLOAT_MAX_HEIGHT: f32 = 0.6;
/// The watermark: pale grey, as wide as this share of the text area, within these sizes.
const WATERMARK_COLOR: Color = Color { r: 0xd9, g: 0xd9, b: 0xd9 };
const WATERMARK_WIDTH: f32 = 0.85;
const WATERMARK_SIZES: (f32, f32) = (24.0, 144.0);

/// A picture the following paragraphs flow around, and how much of its height is still beside them.
#[derive(Clone, Debug, PartialEq)]
pub struct Around {
    pub right: bool,
    /// Width the picture takes from the lines beside it, gap included.
    pub inset: f32,
    /// Height still to pass, from the bottom of the previous paragraph.
    pub remaining: f32,
    /// Space after the previous paragraph.
    pub after: f32,
    /// The picture itself, for the first paragraph: it is placed with that paragraph's first line.
    pub float: Option<BoxFlow>,
}

/// The floating picture `block` makes in a text area `width` wide, if it is one.
pub fn float_of(block: &Block, width: f32, page_height: f32) -> Option<Around> {
    let Block::Leaf(l) = block else { return None };
    let right = match l.align {
        Some(Align::Right) => true,
        Some(Align::Left) => false,
        _ => return None,
    };
    if l.kind != LeafKind::Image || !l.wrap || l.width <= 0.0 || l.height <= 0.0 {
        return None;
    }
    if l.width > width * FLOAT_MAX_WIDTH || l.height > page_height * FLOAT_MAX_HEIGHT {
        return None;
    }
    let float = BoxFlow {
        pos: 0,
        kind: l.kind,
        x: if right { width - l.width } else { 0.0 },
        w: l.width,
        h: l.height,
        space_before: 0.0,
        space_after: 0.0,
        data: l.data.clone(),
        label: None,
    };
    Some(Around {
        right,
        inset: l.width + FLOAT_GAP,
        remaining: l.height + FLOAT_GAP / 2.0,
        after: 0.0,
        float: Some(float),
    })
}

/// Spacing around pictures, formulas on their own line, code blocks, quotes and tables.
const BLOCK_GAP: f32 = 6.0;
const RULE_GAP: f32 = 12.0;
const RULE_THICKNESS: f32 = 0.75;
/// Size of a picture whose dimensions are not known yet.
const UNKNOWN_IMAGE: f32 = 120.0;
/// Distance of the header's top and the footer's bottom from the page edge (Word's default 1.25 cm).
const RUNNING_EDGE: f32 = 35.4;

#[derive(Clone, Debug, PartialEq)]
pub enum FlowItem {
    Para(ParaFlow),
    Box(BoxFlow),
    Table(TableFlow),
    Toc(TocFlow),
    PageBreak { pos: u32 },
}

/// A heading of the document, as the table of contents needs it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    /// Document position of the heading's content start.
    pub pos: u32,
    /// The list the entry belongs to (`figure`, `table`); `None` for a real heading.
    pub list: Option<String>,
}

/// The table of contents: its lines are known before the pages are (the page numbers and dot leaders are added
/// once they are).
#[derive(Clone, Debug, PartialEq)]
pub struct TocFlow {
    /// Position of the contents block, relative to its top-level block.
    pub pos: u32,
    pub x: f32,
    pub width: f32,
    pub title: Vec<Arc<Line>>,
    pub rows: Vec<TocRow>,
    pub space_before: f32,
    pub space_after: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TocRow {
    /// Index of the heading in the document's list of headings.
    pub heading: usize,
    pub lines: Vec<Arc<Line>>,
}

/// Where a contents row ended up: the page number and the dots go on its last line.
#[derive(Clone, Debug, PartialEq)]
pub struct TocMark {
    pub heading: usize,
    /// Top of the row's last line, the left edge of the contents box, and its width.
    pub y: f32,
    pub x: f32,
    pub width: f32,
    /// Right end of the row's text, from `x`.
    pub text_end: f32,
}

/// A footnote: the line of its paragraph that carries the mark, and the note's own lines (as wide as the page's
/// text area, wherever the mark is).
#[derive(Clone, Debug, PartialEq)]
pub struct NoteFlow {
    pub line: usize,
    pub lines: Vec<Arc<Line>>,
    pub height: f32,
}

/// Space the footnote area takes above its first note: a gap, the separator rule, a gap.
pub const NOTE_SEPARATOR: f32 = 10.0;
const NOTE_RULE_OFFSET: f32 = 6.0;
/// Room kept at the right of a contents row for the page number.
const TOC_NUMBER_COLUMN: f32 = 28.0;
const TOC_LEVEL_INDENT: f32 = 35.433_07;
const TOC_GAP: f32 = 4.0;
/// Most dots in one table-of-contents leader. A leader this long is about 35,000 pt wide, far beyond any page the
/// engine can render; the cap only keeps a finite but absurd page width from asking for a huge allocation.
const MAX_LEADER_DOTS: f32 = 10_000.0;

/// Cell padding left and right (Word's default 0.19 cm) and above and below the content.
pub const CELL_PAD_X: f32 = 5.4;
pub const CELL_PAD_Y: f32 = 2.0;
/// From the top of a row to its content: the grid line takes its own thickness, then the padding. Without the
/// line's share every row came out half a point lower than in Word, LibreOffice and a browser.
pub const CELL_TOP: f32 = TABLE_BORDER + CELL_PAD_Y;
/// What a row is taller than its content by.
pub const CELL_EXTRA: f32 = CELL_TOP + CELL_PAD_Y;
/// Thickness of the table grid.
pub const TABLE_BORDER: f32 = 0.5;
const MIN_COLUMN: f32 = 24.0;
/// More columns than this are not laid out as a grid (a damaged document, not a real table).
const MAX_COLUMNS: usize = 63;

/// A table with its columns sized and every cell's content already broken into lines.
#[derive(Clone, Debug, PartialEq)]
pub struct TableFlow {
    /// Left edge of the table, from the left edge of the text area.
    pub x: f32,
    /// Left edge of every column from `x`, then the table's right edge: one more entry than columns.
    pub cols: Vec<f32>,
    pub rows: Vec<RowFlow>,
    /// Leading rows made of header cells: repeated at the top of every page the table continues on.
    pub header_rows: usize,
    pub borders: Borders,
    pub space_before: f32,
    pub space_after: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RowFlow {
    /// The cells that start in this row (a cell spanning rows belongs to its first).
    pub cells: Vec<CellFlow>,
    pub height: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CellFlow {
    pub col: usize,
    pub colspan: usize,
    pub rowspan: usize,
    /// Content with `x` and positions already those of the cell.
    pub items: Vec<FlowItem>,
    /// Height the content needs, padding included.
    pub height: f32,
    pub background: Option<Color>,
    pub valign: VAlign,
}

impl TableFlow {
    pub fn height(&self) -> f32 {
        self.rows.iter().map(|r| r.height).sum()
    }
    fn rows_height(&self, from: usize, to: usize) -> f32 {
        self.rows.iter().take(to).skip(from).map(|r| r.height).sum()
    }
    /// End of the group of rows tied together by row-spanning cells that starts at row `from`.
    fn group_end(&self, from: usize) -> usize {
        let mut end = from + 1;
        let mut r = from;
        while r < end.min(self.rows.len()) {
            if let Some(row) = self.rows.get(r) {
                for c in &row.cells {
                    end = end.max(r + c.rowspan);
                }
            }
            r += 1;
        }
        end.min(self.rows.len())
    }
}

/// Height of items stacked from the top of a box: the first item's space before is dropped, as at a page top.
fn stack_height(items: &[FlowItem]) -> f32 {
    let mut h = 0.0;
    let mut pending = 0.0f32;
    let mut first = true;
    for item in items {
        if matches!(item, FlowItem::PageBreak { .. }) {
            continue;
        }
        let (before, body, after) = parts(item);
        h += if first { 0.0 } else { pending.max(before) } + body;
        pending = after;
        first = false;
    }
    h
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParaFlow {
    pub lines: Vec<Arc<Line>>,
    /// Left edge and width of the paragraph's box, from the left edge of the text area.
    pub x: f32,
    pub width: f32,
    /// Position of the paragraph's content start, relative to its top-level block.
    pub pos: u32,
    pub space_before: f32,
    pub space_after: f32,
    pub keep_next: bool,
    pub keep_lines: bool,
    pub page_break_before: bool,
    /// Fill behind every line (code blocks).
    pub background: Option<Color>,
    /// Footnotes whose marks are in this paragraph, in order.
    pub notes: Vec<NoteFlow>,
    /// A picture placed with the paragraph's first line, the text flowing beside it. Its position is the one
    /// before the paragraph's block.
    pub float: Option<BoxFlow>,
    /// The first lines are shortened for a picture beside them (this paragraph's or an earlier one's).
    pub narrowed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoxFlow {
    /// Position of the leaf block, relative to its top-level block.
    pub pos: u32,
    pub kind: LeafKind,
    pub x: f32,
    pub w: f32,
    pub h: f32,
    pub space_before: f32,
    pub space_after: f32,
    pub data: Option<String>,
    /// A line drawn level with the middle of the box and holding no positions (an equation number), with the left
    /// edge and the width of the text area it is aligned in.
    pub label: Option<(Arc<Line>, f32, f32)>,
}

impl BoxFlow {
    /// The label as a drawn-only line for a box placed with its top at `top` and `h` tall.
    fn label_line(&self, left: f32, top: f32, h: f32) -> Option<PlacedLine> {
        let (line, x, width) = self.label.as_ref()?;
        Some(PlacedLine {
            line: Arc::clone(line),
            x: left + x,
            y: top + (h - line.height) / 2.0,
            box_w: *width,
            base: 0,
            background: None,
            region: 0,
        })
    }
}

/// The flow items of one top-level block, with what they were computed from.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockFlow {
    pub items: Vec<FlowItem>,
    /// Footnote marks inside the block, and how many preceded it when it was laid out.
    pub footnotes: u32,
    pub footnote_base: u32,
    pub width: f32,
    /// Set when the block holds a table of contents: what its rows were made from (a hash of the headings).
    pub toc_key: Option<u64>,
    /// The picture the block was laid out beside, and what is left of it for the next block.
    pub around_in: Option<Around>,
    pub around_out: Option<Around>,
    /// The block is a picture that the next paragraph carries (it has no items of its own).
    pub floated: bool,
}

fn roman(mut n: u32) -> String {
    const T: [(u32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut s = String::new();
    for (v, r) in T {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

/// Bijective base-N numbering with the given letters: a, b, …, z, aa, ab, …
#[expect(clippy::cast_possible_truncation, reason = "a numbering alphabet has a handful of letters")]
fn alphabetic(mut n: u32, letters: &[char]) -> String {
    let base = letters.len() as u32;
    let mut out = Vec::new();
    while n > 0 && base > 0 {
        n -= 1;
        if let Some(c) = letters.get((n % base) as usize) {
            out.push(*c);
        }
        n /= base;
    }
    out.iter().rev().collect()
}

/// The marker of item `n` (from 1) of a list. The same sequences as the editor's CSS and the DOCX export.
pub fn list_marker(list: &List, n: u32) -> String {
    const CYRILLIC: [char; 28] = [
        'а', 'б', 'в', 'г', 'д', 'е', 'ж', 'з', 'и', 'к', 'л', 'м', 'н', 'о', 'п', 'р', 'с', 'т', 'у', 'ф', 'х', 'ц',
        'ч', 'ш', 'щ', 'э', 'ю', 'я',
    ];
    let latin: Vec<char> = ('a'..='z').collect();
    let style = list.style.as_deref().unwrap_or(if list.ordered { "decimal" } else { "disc" });
    match style {
        "circle" => "○".into(),
        "square" => "▪".into(),
        "dash" => "–".into(),
        "decimal-paren" => format!("{n})"),
        "cyrillic-paren" => format!("{})", alphabetic(n, &CYRILLIC)),
        "latin-paren" => format!("{})", alphabetic(n, &latin)),
        "upper-roman" => format!("{}.", roman(n)),
        "decimal" => format!("{n}."),
        _ if list.ordered => format!("{n}."),
        _ => "•".into(),
    }
}

struct Flattener<'a, 'f> {
    sh: &'a Shapers<'f>,
    out: Vec<FlowItem>,
    footnote_base: u32,
    footnotes: u32,
    /// Width of the page's text area: footnotes are laid out in it wherever their mark is.
    page_width: f32,
    env: Env<'a>,
    has_toc: bool,
    /// The picture the block's paragraph flows around (top-level paragraphs only), and what is left after it.
    around: Option<&'a Around>,
    around_out: Option<Around>,
}

/// Every line of a table of contents, top to bottom.
fn toc_lines(t: &TocFlow) -> impl Iterator<Item = &Arc<Line>> {
    t.title.iter().chain(t.rows.iter().flat_map(|r| r.lines.iter()))
}

fn plain(text: &str, marks: sfdoc_model::Marks) -> sfdoc_model::Inline {
    sfdoc_model::Inline::Text(sfdoc_model::Text { text: text.to_owned(), marks })
}

impl Flattener<'_, '_> {
    fn para(&mut self, p: &Paragraph, ctx: Context, x: f32, width: f32, pos: u32, marker: Option<&str>) -> Option<()> {
        let style = para_style(p, ctx, self.env.styles);
        // Beside a picture: the lines above its bottom are shorter.
        let around = self.around.filter(|_| ctx == Context::Body && pos == 0 && !style.code);
        let narrow = around.and_then(|a| {
            let lead = if a.float.is_some() { 0.0 } else { a.after.max(style.space_before) };
            let until = a.remaining - lead;
            (until > 0.0).then_some(Narrow { right: a.right, inset: a.inset, until })
        });
        let inp = ParaInput {
            para: p,
            style: &style,
            width,
            marker: marker.map(|m| (m, -LIST_HANGING)),
            footnote_base: self.footnote_base + self.footnotes,
            narrow,
            hyphenate: self.env.hyphenate && !style.code,
        };
        let lines = layout_paragraph(&inp, self.sh)?;
        if let (Some(a), Some(n)) = (around, narrow) {
            let left = n.until - lines.iter().map(|l| l.height).sum::<f32>();
            if left > 0.0 {
                self.around_out = Some(Around { remaining: left, after: style.space_after, float: None, ..a.clone() });
            }
        }
        // Footnotes: which line carries each mark, and the note's text laid out for the bottom of the page.
        let mut notes = Vec::new();
        let mut offset = 0u32;
        for inline in &p.inlines {
            match inline {
                sfdoc_model::Inline::Text(t) => offset += sfdoc_model::utf16_len(&t.text),
                sfdoc_model::Inline::Atom(a) => {
                    if a.kind == sfdoc_model::AtomKind::Footnote {
                        self.footnotes += 1;
                        let number = (self.footnote_base + self.footnotes).to_string();
                        let sup = sfdoc_model::Marks { vert: sfdoc_model::VertAlign::Sup, ..Default::default() };
                        let body = format!(" {}", a.data.as_deref().unwrap_or(""));
                        let note = Paragraph {
                            inlines: vec![plain(&number, sup), plain(&body, Default::default())],
                            ..Default::default()
                        };
                        let note_style = para_style(&note, Context::Footnote, self.env.styles);
                        let note_inp = ParaInput {
                            para: &note,
                            style: &note_style,
                            width: self.page_width,
                            marker: None,
                            footnote_base: 0,
                            narrow: None,
                            hyphenate: false,
                        };
                        let note_lines: Vec<Arc<Line>> =
                            layout_paragraph(&note_inp, self.sh)?.into_iter().map(Arc::new).collect();
                        let line = lines
                            .iter()
                            .position(|l| offset >= l.from && offset < l.to)
                            .unwrap_or(lines.len().saturating_sub(1));
                        let height = note_lines.iter().map(|l| l.height).sum();
                        notes.push(NoteFlow { line, lines: note_lines, height });
                    }
                    offset += 1;
                }
            }
        }
        let code = style.code;
        self.out.push(FlowItem::Para(ParaFlow {
            lines: lines.into_iter().map(Arc::new).collect(),
            x,
            width,
            pos: pos + 1,
            space_before: style.space_before + if code { BLOCK_GAP } else { 0.0 },
            space_after: style.space_after + if code { BLOCK_GAP } else { 0.0 },
            keep_next: style.keep_next,
            keep_lines: style.keep_lines,
            page_break_before: style.page_break_before,
            background: code.then_some(CODE_BLOCK_COLOR),
            notes,
            float: around.and_then(|a| a.float.clone()),
            narrowed: narrow.is_some(),
        }));
        Some(())
    }

    /// The table of contents: a centred bold title (the leaf's `data`) and one row per heading, indented by level.
    fn toc(&mut self, l: &Leaf, x: f32, width: f32, pos: u32) -> Option<()> {
        self.has_toc = true;
        let font = body_font(self.env.styles);
        let base = |align: Align, left: f32, right: f32, bold: bool| ParaStyle {
            left,
            right,
            ..ParaStyle::plain(align, 1.5, font, BODY_SIZE, bold)
        };
        let lines_of = |text: &str, style: &ParaStyle, sh: &Shapers<'_>| -> Option<Vec<Arc<Line>>> {
            let para = Paragraph { inlines: vec![plain(text, Default::default())], ..Default::default() };
            let inp =
                ParaInput { para: &para, style, width, marker: None, footnote_base: 0, narrow: None, hyphenate: false };
            Some(layout_paragraph(&inp, sh)?.into_iter().map(Arc::new).collect())
        };
        let title = match l.data.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            Some(t) => lines_of(t, &base(Align::Center, 0.0, 0.0, true), self.sh)?,
            None => Vec::new(),
        };
        let mut rows = Vec::new();
        // A list of figures or tables shows the entries of its list; the table of contents shows the headings.
        for (i, h) in self.env.headings.iter().enumerate().filter(|(_, h)| h.list == l.list) {
            let indent = (f32::from(h.level.clamp(1, 3)) - 1.0) * TOC_LEVEL_INDENT;
            let style = base(Align::Left, indent.min(width / 2.0), TOC_NUMBER_COLUMN.min(width / 4.0), false);
            rows.push(TocRow { heading: i, lines: lines_of(&h.text, &style, self.sh)? });
        }
        self.out.push(FlowItem::Toc(TocFlow {
            pos,
            x,
            width,
            title,
            rows,
            space_before: BLOCK_GAP,
            space_after: 2.0 * BLOCK_GAP,
        }));
        Some(())
    }

    fn leaf(&mut self, l: &Leaf, x: f32, width: f32, pos: u32) {
        // An equation number: one line at the right edge of the text area, in the body font.
        let label = l.label.as_deref().map(str::trim).filter(|t| !t.is_empty()).and_then(|text| {
            let style = ParaStyle::plain(Align::Right, 1.0, body_font(self.env.styles), BODY_SIZE, false);
            let para = Paragraph { inlines: vec![plain(text, Default::default())], ..Default::default() };
            let inp = ParaInput {
                para: &para,
                style: &style,
                width,
                marker: None,
                footnote_base: 0,
                narrow: None,
                hyphenate: false,
            };
            let line = layout_paragraph(&inp, self.sh)?.into_iter().next()?;
            Some((Arc::new(line), x, width))
        });
        let boxed = |w: f32, h: f32, gap: f32, centre: bool| {
            // Too wide for the text area: scale down, keeping the proportions.
            let k = if w > width && w > 0.0 { width / w } else { 1.0 };
            let (w, h) = (w * k, h * k);
            let at = match (centre, l.align) {
                (false, _) | (true, Some(Align::Left | Align::Justify)) => x,
                (true, Some(Align::Right)) => x + (width - w),
                (true, Some(Align::Center) | None) => x + (width - w) / 2.0,
            };
            BoxFlow {
                pos,
                kind: l.kind,
                x: at,
                w,
                h,
                space_before: gap,
                space_after: gap,
                data: l.data.clone(),
                label: if centre { label.clone() } else { None },
            }
        };
        let item = match l.kind {
            LeafKind::PageBreak => FlowItem::PageBreak { pos },
            LeafKind::Image => {
                let (w, h) =
                    if l.width > 0.0 && l.height > 0.0 { (l.width, l.height) } else { (UNKNOWN_IMAGE, UNKNOWN_IMAGE) };
                FlowItem::Box(boxed(w, h, BLOCK_GAP, true))
            }
            LeafKind::Math => FlowItem::Box(boxed(l.width, l.height.max(1.0), BLOCK_GAP, true)),
            LeafKind::Rule => FlowItem::Box(boxed(width, RULE_THICKNESS, RULE_GAP, false)),
            LeafKind::Other => FlowItem::Box(boxed(l.width, l.height, 0.0, false)),
            LeafKind::Toc | LeafKind::Hidden => FlowItem::Box(boxed(0.0, 0.0, 0.0, false)),
        };
        self.out.push(item);
    }

    /// Lay out `blocks` whose first block starts at `pos`; `marker` goes to the first paragraph if it comes first.
    fn blocks(
        &mut self,
        blocks: &[Block],
        ctx: Context,
        x: f32,
        width: f32,
        mut pos: u32,
        mut marker: Option<&str>,
    ) -> Option<()> {
        for b in blocks {
            match b {
                Block::Paragraph(p) => self.para(p, ctx, x, width, pos, marker.take())?,
                Block::Leaf(l) if l.kind == LeafKind::Toc => self.toc(l, x, width, pos)?,
                Block::Leaf(l) => self.leaf(l, x, width, pos),
                Block::List(list) => {
                    let mut at = pos + 1;
                    let (ix, iw) = (x + LIST_INDENT, (width - LIST_INDENT).max(1.0));
                    for (i, item) in list.items.iter().enumerate() {
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "a list never has anywhere near u32::MAX items"
                        )]
                        let m = list_marker(list, list.start.saturating_add(i as u32));
                        self.blocks(&item.blocks, Context::ListItem, ix, iw, at + 1, Some(&m))?;
                        at += 2 + item.blocks.iter().map(Block::size).sum::<u32>();
                    }
                }
                Block::Quote(q) => {
                    let first = self.out.len();
                    let indent = cm(1.25);
                    self.blocks(&q.blocks, Context::Quote, x + indent, (width - indent).max(1.0), pos + 1, None)?;
                    self.pad(first);
                }
                Block::Table(t) => self.table(t, x, width, pos)?,
            }
            marker = None;
            pos += b.size();
        }
        Some(())
    }

    /// Widths the blocks ask for: with everything on one line, and with every allowed line break taken. This is
    /// what "size the columns by content" measures.
    fn natural_widths(&self, blocks: &[Block], ctx: Context) -> Option<(f32, f32)> {
        let (mut max, mut min) = (0.0f32, 0.0f32);
        for b in blocks {
            let (own_max, own_min) = match b {
                Block::Paragraph(p) => crate::para::paragraph_widths(p, &para_style(p, ctx, self.env.styles), self.sh)?,
                // A picture shrinks with its column, so it asks for no minimum.
                Block::Leaf(l) => (l.width.max(0.0), 0.0),
                Block::List(list) => {
                    let mut m = (0.0f32, 0.0f32);
                    for item in &list.items {
                        let w = self.natural_widths(&item.blocks, Context::ListItem)?;
                        m = (m.0.max(w.0), m.1.max(w.1));
                    }
                    (m.0 + LIST_INDENT, m.1 + LIST_INDENT)
                }
                Block::Quote(q) => {
                    let w = self.natural_widths(&q.blocks, Context::Quote)?;
                    (w.0 + cm(1.25), w.1 + cm(1.25))
                }
                Block::Table(t) => {
                    let mut m = (0.0f32, 0.0f32);
                    for row in &t.rows {
                        let mut sum = (0.0, 0.0);
                        for cell in &row.cells {
                            let w = self.natural_widths(&cell.blocks, Context::Cell { header: cell.header })?;
                            sum = (sum.0 + w.0 + 2.0 * CELL_PAD_X, sum.1 + w.1 + 2.0 * CELL_PAD_X);
                        }
                        m = (m.0.max(sum.0), m.1.max(sum.1));
                    }
                    m
                }
            };
            max = max.max(own_max);
            min = min.max(own_min);
        }
        Some((max, min))
    }

    fn table(&mut self, t: &Table, x: f32, width: f32, pos: u32) -> Option<()> {
        // 1. The grid: which column every cell starts in, with row spans taking their slots in the rows below.
        struct Slot<'c> {
            row: usize,
            col: usize,
            colspan: usize,
            rowspan: usize,
            cell: &'c sfdoc_model::Cell,
            pos: u32,
        }
        let nrows = t.rows.len();
        let mut taken: Vec<Vec<bool>> = vec![Vec::new(); nrows];
        let mut slots: Vec<Slot<'_>> = Vec::new();
        let mut ncols = 0usize;
        let mut at = pos + 1;
        for (r, row) in t.rows.iter().enumerate() {
            at += 1;
            let mut c = 0usize;
            for cell in &row.cells {
                while taken.get(r).and_then(|v| v.get(c)).copied().unwrap_or(false) {
                    c += 1;
                }
                let c0 = c.min(MAX_COLUMNS);
                let colspan = (cell.colspan.max(1) as usize).min(MAX_COLUMNS + 1 - c0);
                let rowspan = (cell.rowspan.max(1) as usize).min(nrows - r);
                for rr in r..r + rowspan {
                    if let Some(v) = taken.get_mut(rr) {
                        if v.len() < c0 + colspan {
                            v.resize(c0 + colspan, false);
                        }
                        for slot in v.iter_mut().skip(c0).take(colspan) {
                            *slot = true;
                        }
                    }
                }
                slots.push(Slot { row: r, col: c0, colspan, rowspan, cell, pos: at + 1 });
                ncols = ncols.max(c0 + colspan);
                c = c0 + colspan;
                at += 2 + cell.blocks.iter().map(Block::size).sum::<u32>();
            }
            at += 1;
        }
        if ncols == 0 {
            return Some(());
        }

        // 2. Column widths: the document's own, or by content. Either way the table never exceeds the text area.
        let mut widths: Vec<f32> =
            if t.col_widths.len() == ncols && t.col_widths.iter().all(|w| w.is_finite() && *w > 0.0) {
                t.col_widths.clone()
            } else {
                // Like a browser's automatic table layout: every column gets at least its widest unbreakable
                // word, the rest of the width is shared in proportion to how much more each column would like.
                let mut max = vec![MIN_COLUMN; ncols];
                let mut min = vec![MIN_COLUMN; ncols];
                for s in slots.iter().filter(|s| s.colspan == 1) {
                    let (a, b) = self.natural_widths(&s.cell.blocks, Context::Cell { header: s.cell.header })?;
                    let pad = 2.0 * CELL_PAD_X + 1.0;
                    if let (Some(mx), Some(mn)) = (max.get_mut(s.col), min.get_mut(s.col)) {
                        *mx = mx.max((a + pad).min(width));
                        *mn = mn.max((b + pad).min(width));
                    }
                }
                let (sum_max, sum_min): (f32, f32) = (max.iter().sum(), min.iter().sum());
                if sum_max <= width {
                    // Room to spare: the table fills the text area, columns keep their proportions.
                    max.iter().map(|n| n * width / sum_max).collect()
                } else if sum_min >= width {
                    // Even the longest words do not fit: shrink everything alike (words will break).
                    min.iter().map(|n| n * width / sum_min).collect()
                } else {
                    let share = (width - sum_min) / (sum_max - sum_min);
                    max.iter().zip(&min).map(|(mx, mn)| mn + (mx - mn) * share).collect()
                }
            };
        let sum: f32 = widths.iter().sum();
        if sum > width {
            for w in &mut widths {
                *w *= width / sum;
            }
        }
        let mut cols = Vec::with_capacity(ncols + 1);
        let mut edge = 0.0f32;
        cols.push(0.0);
        for w in &widths {
            edge += w;
            cols.push(edge);
        }

        // 3. Every cell's content, laid out in its own width.
        let mut rows: Vec<RowFlow> = (0..nrows).map(|_| RowFlow { cells: Vec::new(), height: 0.0 }).collect();
        for s in &slots {
            let left = cols.get(s.col).copied().unwrap_or(0.0);
            let right = cols.get(s.col + s.colspan).copied().unwrap_or(edge);
            let inner = (right - left - 2.0 * CELL_PAD_X).max(1.0);
            let outer = std::mem::take(&mut self.out);
            let done = self.blocks(
                &s.cell.blocks,
                Context::Cell { header: s.cell.header },
                x + left + CELL_PAD_X,
                inner,
                s.pos,
                None,
            );
            let items = std::mem::replace(&mut self.out, outer);
            done?;
            let height = stack_height(&items) + CELL_EXTRA;
            if let Some(row) = rows.get_mut(s.row) {
                row.cells.push(CellFlow {
                    col: s.col,
                    colspan: s.colspan,
                    rowspan: s.rowspan,
                    items,
                    height,
                    background: s.cell.background,
                    valign: s.cell.valign.unwrap_or_default(),
                });
            }
        }

        // 4. Row heights: the tallest single-row cell; a cell spanning rows stretches the last of them if it has to.
        let base = self
            .sh
            .fonts()
            .resolve(None, false, false)
            .and_then(|r| self.sh.fonts().get(r.id))
            .map_or(16.0, |f| f.metrics().line_height(crate::style::BODY_SIZE));
        for row in &mut rows {
            row.height = row.cells.iter().filter(|c| c.rowspan == 1).map(|c| c.height).fold(0.0, f32::max);
        }
        for r in 0..nrows {
            let spans: Vec<(usize, f32)> = rows
                .get(r)
                .map(|row| row.cells.iter().filter(|c| c.rowspan > 1).map(|c| (c.rowspan, c.height)).collect())
                .unwrap_or_default();
            for (rowspan, height) in spans {
                let have: f32 = rows.iter().skip(r).take(rowspan).map(|x| x.height).sum();
                if height > have
                    && let Some(last) = rows.get_mut(r + rowspan - 1)
                {
                    last.height += height - have;
                }
            }
        }
        for row in &mut rows {
            // A row no cell starts in (everything in it is spanned from above) still takes a line.
            if row.height <= 0.0 {
                row.height = base + CELL_EXTRA;
            }
        }

        // 5. Header rows: leading rows of header cells only, none of which reaches below them.
        let mut header_rows = 0;
        for (r, row) in t.rows.iter().enumerate() {
            if row.cells.is_empty() || !row.cells.iter().all(|c| c.header) {
                break;
            }
            header_rows = r + 1;
        }
        while header_rows > 0
            && rows
                .iter()
                .take(header_rows)
                .enumerate()
                .any(|(r, row)| row.cells.iter().any(|c| r + c.rowspan > header_rows))
        {
            header_rows -= 1;
        }
        if header_rows >= nrows {
            header_rows = 0;
        }
        self.out.push(FlowItem::Table(TableFlow {
            x,
            cols,
            rows,
            header_rows,
            borders: t.borders.unwrap_or_default(),
            space_before: BLOCK_GAP,
            space_after: BLOCK_GAP,
        }));
        Some(())
    }

    /// Add the block gap before the first and after the last item produced since `first`.
    fn pad(&mut self, first: usize) {
        let set = |item: Option<&mut FlowItem>, before: bool| match item {
            Some(FlowItem::Para(p)) if before => p.space_before += BLOCK_GAP,
            Some(FlowItem::Para(p)) => p.space_after += BLOCK_GAP,
            Some(FlowItem::Box(b)) if before => b.space_before += BLOCK_GAP,
            Some(FlowItem::Box(b)) => b.space_after += BLOCK_GAP,
            Some(FlowItem::Table(t)) if before => t.space_before += BLOCK_GAP,
            Some(FlowItem::Table(t)) => t.space_after += BLOCK_GAP,
            Some(FlowItem::Toc(t)) if before => t.space_before += BLOCK_GAP,
            Some(FlowItem::Toc(t)) => t.space_after += BLOCK_GAP,
            _ => {}
        };
        if self.out.len() > first {
            set(self.out.get_mut(first), true);
            set(self.out.last_mut(), false);
        }
    }
}

/// A hash of what a table of contents is built from.
pub fn headings_key(headings: &[Heading]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for x in headings {
        (x.level, &x.text, &x.list).hash(&mut h);
    }
    h.finish()
}

/// Flow items of one top-level block for a text area `width` wide. The document's headings are in `env`: a block
/// with a table of contents is built from them. `around` is the picture the block's paragraph flows beside.
pub fn flatten(
    block: &Block,
    width: f32,
    footnote_base: u32,
    env: Env<'_>,
    around: Option<&Around>,
    sh: &Shapers<'_>,
) -> Option<BlockFlow> {
    let mut f = Flattener {
        sh,
        out: Vec::new(),
        footnote_base,
        footnotes: 0,
        page_width: width,
        env,
        has_toc: false,
        around,
        around_out: None,
    };
    f.blocks(std::slice::from_ref(block), Context::Body, 0.0, width, 0, None)?;
    let toc_key = f.has_toc.then(|| headings_key(env.headings));
    Some(BlockFlow {
        items: f.out,
        footnotes: f.footnotes,
        footnote_base,
        width,
        toc_key,
        around_in: around.cloned(),
        around_out: f.around_out,
        floated: false,
    })
}

/// The flow of a picture that the next paragraph carries beside its first lines: nothing of its own.
pub fn floated(width: f32, footnote_base: u32) -> BlockFlow {
    BlockFlow {
        items: Vec::new(),
        footnotes: 0,
        footnote_base,
        width,
        toc_key: None,
        around_in: None,
        around_out: None,
        floated: true,
    }
}

/// The headings of a document in order, wherever they are nested. `pos` is the position of the first block.
pub fn collect_headings(blocks: &[Block], mut pos: u32, out: &mut Vec<Heading>) {
    for b in blocks {
        match b {
            Block::Paragraph(p) if p.level > 0 || p.props.entry_of.is_some() => {
                let mut text = String::new();
                for i in &p.inlines {
                    if let sfdoc_model::Inline::Text(t) = i {
                        text.push_str(&t.text);
                    }
                }
                let text = text.trim();
                if !text.is_empty() {
                    let list = if p.level > 0 { None } else { p.props.entry_of.clone() };
                    out.push(Heading { level: p.level.max(1), text: text.to_owned(), pos: pos + 1, list });
                }
            }
            Block::Paragraph(_) | Block::Leaf(_) => {}
            Block::List(list) => {
                let mut at = pos + 1;
                for item in &list.items {
                    collect_headings(&item.blocks, at + 1, out);
                    at += 2 + item.blocks.iter().map(Block::size).sum::<u32>();
                }
            }
            Block::Quote(q) => collect_headings(&q.blocks, pos + 1, out),
            Block::Table(t) => {
                let mut at = pos + 1;
                for row in &t.rows {
                    at += 1;
                    for cell in &row.cells {
                        collect_headings(&cell.blocks, at + 1, out);
                        at += 2 + cell.blocks.iter().map(Block::size).sum::<u32>();
                    }
                    at += 1;
                }
            }
        }
        pos += b.size();
    }
}

/// A line put on a page.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedLine {
    pub line: Arc<Line>,
    /// Page coordinates of the paragraph box's left edge and of the line box's top.
    pub x: f32,
    pub y: f32,
    pub box_w: f32,
    /// Document position of the paragraph's content start: add a fragment's offsets to get positions.
    pub base: u32,
    pub background: Option<Color>,
    /// The table cell the line is in: an index into the page's `cells` plus one; 0 outside tables.
    pub region: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlacedBox {
    pub rect: Rect,
    pub pos: u32,
    pub kind: LeafKind,
    pub data: Option<String>,
    pub region: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    Line(PlacedLine),
    Box(PlacedBox),
}

impl Entry {
    /// First and last document position the entry holds a caret for.
    pub fn span(&self) -> (u32, u32) {
        match self {
            Entry::Line(l) => (l.base + l.line.from, l.base + l.line.to),
            Entry::Box(b) => (b.pos, b.pos + 1),
        }
    }
    pub fn top(&self) -> f32 {
        match self {
            Entry::Line(l) => l.y,
            Entry::Box(b) => b.rect.y,
        }
    }
    pub fn bottom(&self) -> f32 {
        match self {
            Entry::Line(l) => l.y + l.line.height,
            Entry::Box(b) => b.rect.bottom(),
        }
    }
    pub fn region(&self) -> u16 {
        match self {
            Entry::Line(l) => l.region,
            Entry::Box(b) => b.region,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageLayout {
    /// Lines and boxes in document order. Inside a table that is cell by cell, so not always top to bottom.
    pub entries: Vec<Entry>,
    /// Lines that are drawn but hold no document positions: header, footer, a table header repeated on a
    /// continuation page.
    pub running: Vec<PlacedLine>,
    /// Lines drawn under everything else: the watermark.
    pub under: Vec<PlacedLine>,
    /// Table cells on this page (entries point here through `region`); a nested table's cells come after the
    /// cell that holds it.
    pub cells: Vec<Rect>,
    /// Filled rectangles under the text: cell shading, the table grid, the footnote separator.
    pub decor: Vec<(Rect, Color)>,
    /// Footnotes whose marks are on this page, in order; `finish` lays them out at the bottom.
    pub notes: Vec<Vec<Arc<Line>>>,
    /// Rows of a table of contents on this page that still wait for their page number.
    pub toc_marks: Vec<TocMark>,
}

impl PageLayout {
    /// Height the footnotes take at the bottom of the page.
    pub fn notes_height(&self) -> f32 {
        if self.notes.is_empty() {
            return 0.0;
        }
        NOTE_SEPARATOR + self.notes.iter().flatten().map(|l| l.height).sum::<f32>()
    }
}

/// Height of the footnotes that come with `items` (a cell's or a row's content).
fn notes_in(items: &[FlowItem]) -> f32 {
    items
        .iter()
        .map(|i| match i {
            FlowItem::Para(p) => p.notes.iter().map(|n| n.height).sum::<f32>(),
            FlowItem::Table(t) => t.rows.iter().flat_map(|r| r.cells.iter()).map(|c| notes_in(&c.items)).sum(),
            _ => 0.0,
        })
        .sum()
}

/// Where the placement of a cell's (or any box's) content stopped.
#[derive(Clone, Copy, Debug, Default)]
struct Cursor {
    item: usize,
    line: usize,
    /// Space owed after the last placed item.
    pending: f32,
    started: bool,
}

/// Place `items` from the cursor on, top-down from `y`, while they fit into `room`. With `force` the first piece is
/// placed even if it does not fit, so a page never stays empty. Returns the height used. `ghost` content is only
/// drawn: it goes to `running` and holds no positions.
#[allow(clippy::too_many_arguments)]
fn place_some(
    items: &[FlowItem],
    cur: &mut Cursor,
    start: u32,
    left: f32,
    y: f32,
    room: f32,
    mut force: bool,
    region: u16,
    ghost: bool,
    page: &mut PageLayout,
) -> f32 {
    let mut used = 0.0f32;
    // A cell that continues from the page before starts at the top of its part: no gap above the first piece,
    // just as there is none at the top of a page.
    let mut resumed = true;
    while let Some(item) = items.get(cur.item) {
        let gap = |before: f32, cur: &Cursor, resumed: bool| {
            if cur.started && !resumed { cur.pending.max(before) } else { 0.0 }
        };
        match item {
            FlowItem::PageBreak { .. } => cur.item += 1,
            FlowItem::Para(p) => {
                while let Some(l) = p.lines.get(cur.line) {
                    let g = if cur.line == 0 { gap(p.space_before, cur, resumed) } else { 0.0 };
                    if used + g + l.height > room + 0.01 && !force {
                        return used;
                    }
                    let line = PlacedLine {
                        line: Arc::clone(l),
                        x: left + p.x,
                        y: y + used + g,
                        box_w: p.width,
                        base: start + p.pos,
                        background: p.background,
                        region,
                    };
                    if ghost {
                        page.running.push(line);
                    } else {
                        page.entries.push(Entry::Line(line));
                        page.notes.extend(p.notes.iter().filter(|n| n.line == cur.line).map(|n| n.lines.clone()));
                    }
                    used += g + l.height;
                    cur.line += 1;
                    cur.started = true;
                    cur.pending = 0.0;
                    force = false;
                    resumed = false;
                }
                cur.item += 1;
                cur.line = 0;
                cur.pending = p.space_after;
            }
            FlowItem::Box(b) => {
                let g = gap(b.space_before, cur, resumed);
                if used + g + b.h > room + 0.01 && !force {
                    return used;
                }
                // Forced onto a page it is too tall for (nothing else is on it): scale it to what there is.
                let avail = room - used - g;
                let k = if b.h > avail + 0.01 && avail.is_finite() && avail > 1.0 { avail / b.h } else { 1.0 };
                let (w, h) = (b.w * k, b.h * k);
                if !ghost {
                    page.entries.push(Entry::Box(PlacedBox {
                        rect: Rect::new(left + b.x + (b.w - w) / 2.0, y + used + g, w, h),
                        pos: start + b.pos,
                        kind: b.kind,
                        data: b.data.clone(),
                        region,
                    }));
                    if let Some(label) = b.label_line(left, y + used + g, h) {
                        page.running.push(label);
                    }
                }
                used += g + h;
                cur.item += 1;
                cur.started = true;
                cur.pending = b.space_after;
                force = false;
                resumed = false;
            }
            FlowItem::Toc(t) => {
                // Inside a cell a table of contents is one unsplittable block.
                let g = gap(t.space_before, cur, resumed);
                let h: f32 = toc_lines(t).map(|l| l.height).sum();
                if used + g + h > room + 0.01 && !force {
                    return used;
                }
                let top = y + used + g;
                if !ghost {
                    place_toc_lines(t, 0, usize::MAX, start, left, top, region, page);
                }
                used += g + h;
                cur.item += 1;
                cur.started = true;
                cur.pending = t.space_after;
                force = false;
                resumed = false;
            }
            FlowItem::Table(t) => {
                // A table inside a cell goes row by row (`cur.line` is the next row): what does not fit continues
                // where the cell does. Rows tied by a spanning cell stay together.
                while cur.line < t.rows.len() {
                    let end = t.group_end(cur.line);
                    let g = if cur.line == 0 { gap(t.space_before, cur, resumed) } else { 0.0 };
                    let h = t.rows_height(cur.line, end);
                    if used + g + h > room + 0.01 && !force {
                        return used;
                    }
                    place_rows(t, cur.line, end, start, left, y + used + g, ghost, page);
                    used += g + h;
                    cur.line = end;
                    cur.started = true;
                    cur.pending = 0.0;
                    force = false;
                    resumed = false;
                }
                cur.item += 1;
                cur.line = 0;
                cur.pending = t.space_after;
            }
        }
    }
    used
}

/// Draw lines `from..to` of a table of contents (title lines first) from `y` down; returns the height used. The
/// lines hold no positions of their own: the whole block is one position, so each drawn part gets one box entry.
#[allow(clippy::too_many_arguments)]
fn place_toc_lines(
    t: &TocFlow,
    from: usize,
    to: usize,
    start: u32,
    left: f32,
    y: f32,
    region: u16,
    page: &mut PageLayout,
) -> f32 {
    let mut top = y;
    let mut index = 0usize;
    let mut draw = |l: &Arc<Line>, mark: Option<usize>, page: &mut PageLayout| {
        if index >= from && index < to {
            page.running.push(PlacedLine {
                line: Arc::clone(l),
                x: left + t.x,
                y: top,
                box_w: t.width,
                base: 0,
                background: None,
                region: 0,
            });
            if let Some(heading) = mark {
                page.toc_marks.push(TocMark {
                    heading,
                    y: top,
                    x: left + t.x,
                    width: t.width,
                    text_end: l.x + l.width,
                });
            }
            top += l.height;
        }
        index += 1;
    };
    for l in &t.title {
        draw(l, None, page);
    }
    for row in &t.rows {
        let last = row.lines.len().saturating_sub(1);
        for (i, l) in row.lines.iter().enumerate() {
            draw(l, (i == last).then_some(row.heading), page);
        }
    }
    if top > y {
        page.entries.push(Entry::Box(PlacedBox {
            rect: Rect::new(left + t.x, y, t.width, top - y),
            pos: start + t.pos,
            kind: LeafKind::Toc,
            data: None,
            region,
        }));
    }
    top - y
}

/// Which sides of a cell that starts in row `r` are drawn: top, bottom, left, right.
fn cell_edges(t: &TableFlow, r: usize, c: &CellFlow) -> [bool; 4] {
    let first_row = r == 0;
    let last_row = r + c.rowspan >= t.rows.len();
    let first_col = c.col == 0;
    let last_col = c.col + c.colspan + 1 >= t.cols.len();
    match t.borders {
        Borders::All => [true; 4],
        Borders::Horizontal => [true, true, false, false],
        Borders::Outer => [first_row, last_row, first_col, last_col],
        Borders::None => [false; 4],
    }
}

/// The frame and shading of one cell.
fn cell_decor(rect: Rect, background: Option<Color>, edges: [bool; 4], page: &mut PageLayout) {
    if let Some(bg) = background {
        page.decor.push((rect, bg));
    }
    let b = TABLE_BORDER;
    let sides = [
        Rect::new(rect.x - b / 2.0, rect.y - b / 2.0, rect.w + b, b),
        Rect::new(rect.x - b / 2.0, rect.bottom() - b / 2.0, rect.w + b, b),
        Rect::new(rect.x - b / 2.0, rect.y - b / 2.0, b, rect.h + b),
        Rect::new(rect.right() - b / 2.0, rect.y - b / 2.0, b, rect.h + b),
    ];
    for (r, on) in sides.into_iter().zip(edges) {
        if on {
            page.decor.push((r, Color::BLACK));
        }
    }
}

fn cell_rect(t: &TableFlow, c: &CellFlow, left: f32, y: f32, h: f32) -> Rect {
    let x0 = t.cols.get(c.col).copied().unwrap_or(0.0);
    let x1 = t.cols.get(c.col + c.colspan).copied().or(t.cols.last().copied()).unwrap_or(x0);
    Rect::new(left + t.x + x0, y, (x1 - x0).max(0.0), h)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "page.cells is checked just above: it holds fewer than u16::MAX entries"
)]
fn open_region(rect: Rect, ghost: bool, page: &mut PageLayout) -> u16 {
    if ghost || page.cells.len() >= usize::from(u16::MAX) - 1 {
        return 0;
    }
    page.cells.push(rect);
    page.cells.len() as u16
}

/// Place rows `from..to` whole, the first one's top at `y`. A cell reaching below `to` is cut there.
#[allow(clippy::too_many_arguments)]
fn place_rows(
    t: &TableFlow,
    from: usize,
    to: usize,
    start: u32,
    left: f32,
    y: f32,
    ghost: bool,
    page: &mut PageLayout,
) -> f32 {
    let mut top = y;
    for r in from..to.min(t.rows.len()) {
        let Some(row) = t.rows.get(r) else { break };
        for c in &row.cells {
            let rect = cell_rect(t, c, left, top, t.rows_height(r, (r + c.rowspan).min(to)));
            cell_decor(rect, c.background, cell_edges(t, r, c), page);
            let region = open_region(rect, ghost, page);
            // A cell taller than its content (a taller neighbour, a merged cell): the content may stand lower.
            let spare = (rect.h - c.height).max(0.0);
            let down = match c.valign {
                VAlign::Top => 0.0,
                VAlign::Middle => spare / 2.0,
                VAlign::Bottom => spare,
            };
            place_some(
                &c.items,
                &mut Cursor::default(),
                start,
                left,
                top + CELL_TOP + down,
                f32::INFINITY,
                true,
                region,
                ghost,
                page,
            );
        }
        top += row.height;
    }
    top - y
}

struct Paginator<'a> {
    page: &'a PageSetup,
    pages: Vec<PageLayout>,
    y: f32,
    /// Space owed after the previous item; dropped at a page break.
    pending: f32,
    /// Bottom of the picture the text of this page flows around; 0 when there is none.
    float_bottom: f32,
}

impl Paginator<'_> {
    fn top(&self) -> f32 {
        self.page.margin_top
    }
    /// Bottom of the text area, footnotes not counted.
    fn floor(&self) -> f32 {
        (self.page.height - self.page.margin_bottom).max(self.top() + 1.0)
    }
    /// How far down text may go on the current page: above the footnotes already on it.
    fn bottom(&self) -> f32 {
        self.floor() - self.pages.last().map_or(0.0, PageLayout::notes_height)
    }
    /// Extra room footnotes `h` tall would take on the current page (the separator comes with the first).
    fn note_cost(&self, h: f32) -> f32 {
        if h <= 0.0 {
            0.0
        } else if self.pages.last().is_some_and(|p| p.notes.is_empty()) {
            h + NOTE_SEPARATOR
        } else {
            h
        }
    }
    fn at_top(&self) -> bool {
        self.pages.last().is_none_or(|p| p.entries.is_empty())
    }
    fn new_page(&mut self) {
        self.pages.push(PageLayout::default());
        self.y = self.top();
        self.pending = 0.0;
        self.float_bottom = 0.0;
    }
    /// What does not flow beside a picture starts under it.
    fn clear_float(&mut self) {
        if self.y < self.float_bottom {
            self.y = self.float_bottom;
        }
        self.float_bottom = 0.0;
    }
    fn push(&mut self, e: Entry) {
        if let Some(p) = self.pages.last_mut() {
            p.entries.push(e);
        }
    }
    /// Gap before an item with `space_before`: nothing at the top of a page; otherwise the larger of the space
    /// after the item above and the space before this one. The two are not added - as in Word and as margins
    /// collapse in a browser.
    fn gap(&self, space_before: f32) -> f32 {
        if self.at_top() { 0.0 } else { self.pending.max(space_before) }
    }
}

/// Space before, height and space after of an item.
fn parts(item: &FlowItem) -> (f32, f32, f32) {
    match item {
        FlowItem::Para(p) => (p.space_before, p.lines.iter().map(|l| l.height).sum::<f32>(), p.space_after),
        FlowItem::Box(b) => (b.space_before, b.h, b.space_after),
        FlowItem::Table(t) => (t.space_before, t.height(), t.space_after),
        FlowItem::Toc(t) => (t.space_before, toc_lines(t).map(|l| l.height).sum::<f32>(), t.space_after),
        FlowItem::PageBreak { .. } => (0.0, 0.0, 0.0),
    }
}

/// Height of the beginning of an item that must stay with a "keep with next" item before it, so that the latter
/// is not left alone at the bottom of a page.
fn first_chunk(item: &FlowItem) -> f32 {
    match item {
        FlowItem::Para(p) => {
            let n = if p.keep_lines { p.lines.len() } else { p.lines.len().min(2) };
            p.lines.iter().take(n).map(|l| l.height).sum::<f32>()
        }
        FlowItem::Box(b) => b.h,
        // The header rows and the first row under them.
        FlowItem::Table(t) => t.rows_height(0, t.group_end(t.header_rows)),
        FlowItem::Toc(t) => toc_lines(t).take(2).map(|l| l.height).sum::<f32>(),
        FlowItem::PageBreak { .. } => 0.0,
    }
}

/// Stack flow items onto pages. `flows` are the top-level blocks in order with their start positions.
pub fn paginate(page: &PageSetup, flows: &[(u32, Arc<BlockFlow>)]) -> Vec<PageLayout> {
    let mut pg = Paginator { page, pages: Vec::new(), y: 0.0, pending: 0.0, float_bottom: 0.0 };
    pg.new_page();
    let left = page.margin_left;
    let items: Vec<(u32, &FlowItem)> =
        flows.iter().flat_map(|(start, f)| f.items.iter().map(move |i| (*start, i))).collect();
    let page_h = pg.floor() - pg.top();

    for (idx, (start, item)) in items.iter().enumerate() {
        // Keep with next: this item and everything chained to it must be followed, on the same page, by the
        // beginning of the first item that is not chained.
        let keeps = |i: &FlowItem| matches!(i, FlowItem::Para(p) if p.keep_next);
        if keeps(item) && !pg.at_top() {
            let mut need = 0.0f32;
            let mut pending = pg.pending;
            let mut j = idx;
            while let Some((_, it)) = items.get(j) {
                let (before, body, after) = parts(it);
                need += pending.max(before);
                if keeps(it) && j - idx < 4 {
                    need += body;
                    pending = after;
                    j += 1;
                } else {
                    need += first_chunk(it);
                    break;
                }
            }
            if need <= page_h && pg.y + need > pg.bottom() + 0.01 {
                pg.new_page();
            }
        }
        if !matches!(item, FlowItem::Para(p) if p.narrowed && p.float.is_none()) {
            pg.clear_float();
        }
        match item {
            FlowItem::PageBreak { pos } => {
                // The break itself is a zero-height box where it happens, so a caret can stand before it.
                let y = pg.y + pg.pending;
                pg.push(Entry::Box(PlacedBox {
                    rect: Rect::new(left, y.min(pg.bottom()), 0.0, 0.0),
                    pos: start + pos,
                    kind: LeafKind::PageBreak,
                    data: None,
                    region: 0,
                }));
                pg.new_page();
            }
            FlowItem::Box(b) => {
                // Taller than a page: scale to the page.
                let k = if b.h > page_h { page_h / b.h } else { 1.0 };
                let (w, h) = (b.w * k, b.h * k);
                if !pg.at_top() && pg.y + pg.gap(b.space_before) + h > pg.bottom() + 0.01 {
                    pg.new_page();
                }
                let y = pg.y + pg.gap(b.space_before);
                let x = left + b.x + (b.w - w) / 2.0;
                pg.push(Entry::Box(PlacedBox {
                    rect: Rect::new(x, y, w, h),
                    pos: start + pos_of(b),
                    kind: b.kind,
                    data: b.data.clone(),
                    region: 0,
                }));
                if let (Some(label), Some(page)) = (b.label_line(left, y, h), pg.pages.last_mut()) {
                    page.running.push(label);
                }
                pg.y = y + h;
                pg.pending = b.space_after;
            }
            FlowItem::Table(t) => {
                let n = t.rows.len();
                let header_h = t.rows_height(0, t.header_rows);
                // After a page break inside the table: the header rows again, drawn only.
                let continue_on_new_page = |pg: &mut Paginator<'_>, r: usize, next_h: f32| {
                    pg.new_page();
                    if r >= t.header_rows && header_h > 0.0 && header_h + next_h <= page_h + 0.01 {
                        let y = pg.y;
                        if let Some(page) = pg.pages.last_mut() {
                            place_rows(t, 0, t.header_rows, *start, left, y, true, page);
                        }
                        pg.y += header_h;
                    }
                };
                let mut r = 0usize;
                while r < n {
                    let end = t.group_end(r);
                    let h = t.rows_height(r, end);
                    let gap = if r == 0 { pg.gap(t.space_before) } else { 0.0 };
                    // Footnotes of these rows come to the same page: they need their room at its bottom.
                    let notes: f32 = t
                        .rows
                        .iter()
                        .take(end)
                        .skip(r)
                        .flat_map(|row| row.cells.iter())
                        .map(|c| notes_in(&c.items))
                        .sum();
                    let fresh_room = page_h
                        - if r >= t.header_rows { header_h } else { 0.0 }
                        - if notes > 0.0 { notes + NOTE_SEPARATOR } else { 0.0 };
                    if pg.y + gap + h + pg.note_cost(notes) <= pg.bottom() + 0.01 {
                        let y = pg.y + gap;
                        if let Some(page) = pg.pages.last_mut() {
                            place_rows(t, r, end, *start, left, y, false, page);
                        }
                        pg.y = y + h;
                        r = end;
                    } else if h <= fresh_room + 0.01 && (!pg.at_top() || pg.y > pg.top() + 0.01) {
                        // The rows fit on a page of their own: move them there whole.
                        continue_on_new_page(&mut pg, r, h);
                    } else if end - r > 1 {
                        // Rows tied by a spanning cell and taller than a page: break between the rows after all.
                        let row_h = t.rows_height(r, r + 1);
                        if pg.y + gap + row_h > pg.bottom() + 0.01 && pg.y > pg.top() + 0.01 {
                            continue_on_new_page(&mut pg, r, row_h);
                        }
                        let y = pg.y + if pg.y > pg.top() + 0.01 { gap } else { 0.0 };
                        if let Some(page) = pg.pages.last_mut() {
                            place_rows(t, r, r + 1, *start, left, y, false, page);
                        }
                        pg.y = y + row_h;
                        r += 1;
                    } else {
                        // One row taller than a page: its cells continue line by line on the next pages.
                        let Some(row) = t.rows.get(r) else { break };
                        let mut cursors = vec![Cursor::default(); row.cells.len()];
                        let mut first = true;
                        loop {
                            let y = pg.y + if first { gap } else { 0.0 };
                            let room = pg.bottom() - y - CELL_EXTRA;
                            // Too little room to start the row here: start on the next page.
                            if first && room < CELL_EXTRA + 20.0 && pg.y > pg.top() + 0.01 {
                                continue_on_new_page(&mut pg, r, 40.0);
                                first = false;
                                continue;
                            }
                            let fresh = pg.y <= pg.top() + header_h + 0.01;
                            let bottom = pg.bottom();
                            let mut seg = 0.0f32;
                            let mut unfinished = false;
                            if let Some(page) = pg.pages.last_mut() {
                                let mut regions = Vec::with_capacity(row.cells.len());
                                for (c, cur) in row.cells.iter().zip(cursors.iter_mut()) {
                                    // The region's rectangle is set below, once the segment's height is known.
                                    let region = open_region(Rect::default(), false, page);
                                    let used = place_some(
                                        &c.items,
                                        cur,
                                        *start,
                                        left,
                                        y + CELL_TOP,
                                        room,
                                        fresh,
                                        region,
                                        false,
                                        page,
                                    );
                                    seg = seg.max(used);
                                    unfinished |= cur.item < c.items.len();
                                    regions.push(region);
                                }
                                // A segment that continues reaches the bottom of the page.
                                seg += CELL_EXTRA;
                                if unfinished {
                                    seg = seg.max(bottom - y);
                                }
                                for (c, region) in row.cells.iter().zip(regions) {
                                    let rect = cell_rect(t, c, left, y, seg);
                                    cell_decor(rect, c.background, cell_edges(t, r, c), page);
                                    if let Some(slot) =
                                        usize::from(region).checked_sub(1).and_then(|i| page.cells.get_mut(i))
                                    {
                                        *slot = rect;
                                    }
                                }
                            }
                            pg.y = y + seg;
                            first = false;
                            if !unfinished {
                                break;
                            }
                            continue_on_new_page(&mut pg, r, 40.0);
                        }
                        r = end;
                    }
                }
                pg.pending = t.space_after;
            }
            FlowItem::Toc(t) => {
                let heights: Vec<f32> = toc_lines(t).map(|l| l.height).collect();
                let n = heights.len();
                let mut next = 0usize;
                if n == 0 {
                    // No title and no headings: still a place for the caret.
                    let y = pg.y + pg.gap(t.space_before);
                    pg.push(Entry::Box(PlacedBox {
                        rect: Rect::new(left + t.x, y.min(pg.bottom()), t.width, 0.0),
                        pos: start + t.pos,
                        kind: LeafKind::Toc,
                        data: None,
                        region: 0,
                    }));
                }
                while next < n {
                    let gap = if next == 0 { pg.gap(t.space_before) } else { 0.0 };
                    let room = pg.bottom() - pg.y - gap + 0.01;
                    let mut fit = 0usize;
                    let mut used = 0.0f32;
                    for h in heights.iter().skip(next) {
                        if used + h > room {
                            break;
                        }
                        used += h;
                        fit += 1;
                    }
                    if fit == 0 {
                        if pg.at_top() && pg.y <= pg.top() + 0.01 {
                            fit = 1;
                        } else {
                            pg.new_page();
                            continue;
                        }
                    }
                    let y = pg.y + gap;
                    let mut placed = 0.0;
                    if let Some(page) = pg.pages.last_mut() {
                        placed = place_toc_lines(t, next, next + fit, *start, left, y, 0, page);
                    }
                    pg.y = y + placed;
                    next += fit;
                    if next < n {
                        pg.new_page();
                    }
                }
                pg.pending = t.space_after;
            }
            FlowItem::Para(p) => {
                if p.page_break_before && !pg.at_top() {
                    pg.new_page();
                }
                // A picture beside the first lines needs its whole height on this page.
                if let Some(f) = &p.float
                    && !pg.at_top()
                    && pg.y + pg.gap(p.space_before) + f.h > pg.bottom() + 0.01
                {
                    pg.new_page();
                }
                let mut next = 0usize;
                let n = p.lines.len();
                let note_h = |i: usize| p.notes.iter().filter(|x| x.line == i).map(|x| x.height).sum::<f32>();
                while next < n {
                    let gap = if next == 0 { pg.gap(p.space_before) } else { 0.0 };
                    let room = pg.bottom() - pg.y - gap + 0.01;
                    let mut fit = 0usize;
                    let mut used = 0.0f32;
                    // A line comes with its footnotes: both must fit, the notes at the bottom of the page.
                    let mut notes = 0.0f32;
                    for (i, l) in p.lines.iter().enumerate().skip(next) {
                        let with = notes + note_h(i);
                        // The last line on a page needs room for its text only: the extra leading of 1.5 line
                        // spacing lies under the text and may hang into the bottom margin, as in Word.
                        let text_h = (l.baseline + l.descent).min(l.height);
                        if used + text_h + pg.note_cost(with) > room {
                            break;
                        }
                        used += l.height;
                        notes = with;
                        fit += 1;
                    }
                    let left_over = n - next - fit;
                    if left_over > 0 {
                        // Not everything fits: no single line at the bottom of a page or the top of the next one.
                        if p.keep_lines && next == 0 {
                            fit = 0;
                        } else if left_over < 2 {
                            fit = fit.saturating_sub(2 - left_over);
                        }
                        if next == 0 && fit < 2 {
                            fit = 0;
                        }
                    }
                    if fit == 0 {
                        if pg.at_top() {
                            // Nothing above it and it still does not fit: place a line anyway to make progress.
                            fit = 1;
                        } else {
                            pg.new_page();
                            continue;
                        }
                    }
                    let mut y = pg.y + gap;
                    if next == 0
                        && let Some(f) = &p.float
                    {
                        pg.push(Entry::Box(PlacedBox {
                            rect: Rect::new(left + f.x, y, f.w, f.h),
                            pos: (start + p.pos).saturating_sub(2),
                            kind: f.kind,
                            data: f.data.clone(),
                            region: 0,
                        }));
                        pg.float_bottom = y + f.h;
                    }
                    for (i, l) in p.lines.iter().enumerate().skip(next).take(fit) {
                        pg.push(Entry::Line(PlacedLine {
                            line: Arc::clone(l),
                            x: left + p.x,
                            y,
                            box_w: p.width,
                            base: start + p.pos,
                            background: p.background,
                            region: 0,
                        }));
                        if let Some(page) = pg.pages.last_mut() {
                            page.notes.extend(p.notes.iter().filter(|x| x.line == i).map(|x| x.lines.clone()));
                        }
                        y += l.height;
                    }
                    pg.y = y;
                    next += fit;
                    if next < n {
                        pg.new_page();
                    }
                }
                pg.pending = p.space_after;
            }
        }
    }
    pg.pages
}

fn pos_of(b: &BoxFlow) -> u32 {
    b.pos
}

/// What is known only once the pages are: footnotes go to the bottom of their pages, rows of a table of contents get
/// their dot leaders and page numbers.
pub fn finish(
    pages: &mut [PageLayout],
    page: &PageSetup,
    headings: &[Heading],
    font: &str,
    sh: &Shapers<'_>,
) -> Option<()> {
    let left = page.margin_left;
    let width = (page.width - page.margin_left - page.margin_right).max(1.0);
    let floor = (page.height - page.margin_bottom).max(page.margin_top + 1.0);
    for p in pages.iter_mut() {
        let total = p.notes_height();
        if total <= 0.0 {
            continue;
        }
        // A page overfull with notes (one note taller than the page) starts them at the top margin.
        let top = (floor - total).max(page.margin_top);
        p.decor.push((Rect::new(left, top + NOTE_RULE_OFFSET, width / 3.0, TABLE_BORDER), Color::BLACK));
        let mut y = top + NOTE_SEPARATOR;
        for l in std::mem::take(&mut p.notes).into_iter().flatten() {
            let h = l.height;
            p.running.push(PlacedLine { line: l, x: left, y, box_w: width, base: 0, background: None, region: 0 });
            y += h;
        }
    }

    if pages.iter().all(|p| p.toc_marks.is_empty()) {
        return Some(());
    }
    // The page every heading starts on.
    let mut page_of: Vec<Option<usize>> = vec![None; headings.len()];
    for (i, p) in pages.iter().enumerate() {
        for e in &p.entries {
            if let Entry::Line(l) = e
                && l.line.from == 0
                && let Ok(h) = headings.binary_search_by_key(&l.base, |h| h.pos)
                && let Some(slot) = page_of.get_mut(h)
                && slot.is_none()
            {
                *slot = Some(i + 1);
            }
        }
    }
    let style = |align: Align, left: f32| ParaStyle { left, ..ParaStyle::plain(align, 1.5, font, BODY_SIZE, false) };
    let line_of = |text: &str, st: &ParaStyle, w: f32| -> Option<Option<Line>> {
        let para = Paragraph { inlines: vec![plain(text, Default::default())], ..Default::default() };
        let inp = ParaInput {
            para: &para,
            style: st,
            width: w,
            marker: None,
            footnote_base: 0,
            narrow: None,
            hyphenate: false,
        };
        Some(layout_paragraph(&inp, sh)?.into_iter().next())
    };
    let dot = line_of(".", &style(Align::Left, 0.0), 1.0e4)?.map_or(3.5, |l| l.width).max(0.5);
    for p in pages.iter_mut() {
        for m in std::mem::take(&mut p.toc_marks) {
            let Some(Some(number)) = page_of.get(m.heading).copied() else { continue };
            let Some(num) = line_of(&number.to_string(), &style(Align::Right, 0.0), m.width)? else { continue };
            let num_left = m.width - num.width;
            // Dots from the end of the text to the number, ending flush before it.
            let room = num_left - TOC_GAP - (m.text_end + TOC_GAP);
            let count = (room / dot).floor().min(MAX_LEADER_DOTS);
            if count >= 3.0 {
                let from = num_left - TOC_GAP - count * dot;
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "count is at least 3.0 and at most MAX_LEADER_DOTS"
                )]
                let dots = ".".repeat(count as usize);
                if let Some(dots) = line_of(&dots, &style(Align::Left, from), 1.0e4)? {
                    p.running.push(PlacedLine {
                        line: Arc::new(dots),
                        x: m.x,
                        y: m.y,
                        box_w: m.width,
                        base: 0,
                        background: None,
                        region: 0,
                    });
                }
            }
            p.running.push(PlacedLine {
                line: Arc::new(num),
                x: m.x,
                y: m.y,
                box_w: m.width,
                base: 0,
                background: None,
                region: 0,
            });
        }
    }
    Some(())
}

/// Add header and footer lines to every page (not to a title page), and the watermark under all of them.
pub fn add_running(
    pages: &mut [PageLayout],
    page: &PageSetup,
    running: &Running,
    font: &str,
    sh: &Shapers<'_>,
) -> Option<()> {
    let width = (page.width - page.margin_left - page.margin_right).max(1.0);
    let line = |text: &str, size: f32, align: Align, color: Option<Color>| -> Option<Vec<Line>> {
        let para = Paragraph {
            inlines: vec![plain(text, sfdoc_model::Marks { color, ..Default::default() })],
            ..Default::default()
        };
        let style = ParaStyle::plain(align, 1.0, font, size, false);
        let inp = ParaInput {
            para: &para,
            style: &style,
            width: if color.is_some() { 1.0e5 } else { width },
            marker: None,
            footnote_base: 0,
            narrow: None,
            hyphenate: false,
        };
        layout_paragraph(&inp, sh)
    };
    // The header and footer text is the same on every page: laid out once, shared by all.
    let shared = |text: &str| -> Option<Vec<Arc<Line>>> {
        Some(if text.is_empty() {
            Vec::new()
        } else {
            line(text, 12.0, Align::Left, None)?.into_iter().map(Arc::new).collect()
        })
    };
    let header = shared(&running.header)?;
    let footer_text = shared(&running.footer)?;
    // Even pages have their own when the document says so.
    let header_even = match &running.header_even {
        Some(text) => shared(text)?,
        None => header.clone(),
    };
    let footer_even = match &running.footer_even {
        Some(text) => shared(text)?,
        None => footer_text.clone(),
    };
    // The watermark: one line, sized to span most of the text area, in the middle of the page.
    let watermark = match running.watermark.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        Some(text) => {
            let probe = line(text, 72.0, Align::Left, Some(WATERMARK_COLOR))?.into_iter().next();
            let natural = probe.map_or(0.0, |l| l.width).max(1.0);
            let size = (72.0 * width * WATERMARK_WIDTH / natural).clamp(WATERMARK_SIZES.0, WATERMARK_SIZES.1);
            line(text, size, Align::Left, Some(WATERMARK_COLOR))?.into_iter().next().map(Arc::new)
        }
        None => None,
    };
    for (i, p) in pages.iter_mut().enumerate() {
        if let Some(mark) = &watermark {
            p.under.push(PlacedLine {
                line: Arc::clone(mark),
                x: page.margin_left + (width - mark.width) / 2.0,
                y: (page.height - mark.height) / 2.0,
                box_w: mark.width,
                base: 0,
                background: None,
                region: 0,
            });
        }
        if i == 0 && running.title_page {
            continue;
        }
        let even = (i + 1) % 2 == 0;
        let number = (i + 1).to_string();
        let numbered = |align: Align| -> Option<Vec<Arc<Line>>> {
            Some(line(&number, 14.0, align, None)?.into_iter().map(Arc::new).collect())
        };
        let place = |lines: Vec<Arc<Line>>, y: f32, p: &mut PageLayout| -> f32 {
            let mut y = y;
            for l in lines {
                let h = l.height;
                p.running.push(PlacedLine {
                    line: l,
                    x: page.margin_left,
                    y,
                    box_w: width,
                    base: 0,
                    background: None,
                    region: 0,
                });
                y += h;
            }
            y
        };
        place(if even { header_even.clone() } else { header.clone() }, RUNNING_EDGE, p);
        if running.page_number == PageNumber::TopRight {
            place(numbered(Align::Right)?, RUNNING_EDGE, p);
        }
        // The footer grows upwards from its bottom edge: text first, the number under it.
        let mut footer = if even { footer_even.clone() } else { footer_text.clone() };
        if running.page_number == PageNumber::BottomCenter {
            footer.extend(numbered(Align::Center)?);
        }
        let h: f32 = footer.iter().map(|l| l.height).sum();
        place(footer, page.height - RUNNING_EDGE - h, p);
    }
    Some(())
}
