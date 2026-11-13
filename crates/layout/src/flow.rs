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
use sfdoc_model::{Align, Block, Color, Leaf, LeafKind, List, PageNumber, PageSetup, Paragraph, Running};

use crate::para::{Line, ParaInput, Shapers, layout_paragraph};
use crate::style::{CODE_BLOCK_COLOR, Context, LIST_HANGING, LIST_INDENT, ParaStyle, TextDefaults, para_style};

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
    PageBreak { pos: u32 },
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
}

/// The flow items of one top-level block, with what they were computed from.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockFlow {
    pub items: Vec<FlowItem>,
    /// Footnote marks inside the block, and how many preceded it when it was laid out.
    pub footnotes: u32,
    pub footnote_base: u32,
    pub width: f32,
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
}

impl Flattener<'_, '_> {
    fn para(&mut self, p: &Paragraph, ctx: Context, x: f32, width: f32, pos: u32, marker: Option<&str>) -> Option<()> {
        let style = para_style(p, ctx);
        let inp = ParaInput {
            para: p,
            style: &style,
            width,
            marker: marker.map(|m| (m, -LIST_HANGING)),
            footnote_base: self.footnote_base + self.footnotes,
        };
        let lines = layout_paragraph(&inp, self.sh)?;
        self.footnotes += p
            .inlines
            .iter()
            .filter(|i| matches!(i, sfdoc_model::Inline::Atom(a) if a.kind == sfdoc_model::AtomKind::Footnote))
            .count() as u32;
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
        }));
        Some(())
    }

    fn leaf(&mut self, l: &Leaf, x: f32, width: f32, pos: u32) {
        let boxed = |w: f32, h: f32, gap: f32, centre: bool| {
            // Too wide for the text area: scale down, keeping the proportions.
            let k = if w > width && w > 0.0 { width / w } else { 1.0 };
            let (w, h) = (w * k, h * k);
            BoxFlow {
                pos,
                kind: l.kind,
                x: if centre { x + (width - w) / 2.0 } else { x },
                w,
                h,
                space_before: gap,
                space_after: gap,
                data: l.data.clone(),
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
                Block::Leaf(l) => self.leaf(l, x, width, pos),
                Block::List(list) => {
                    let mut at = pos + 1;
                    let (ix, iw) = (x + LIST_INDENT, (width - LIST_INDENT).max(1.0));
                    for (i, item) in list.items.iter().enumerate() {
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
                Block::Table(t) => {
                    // Until the table stage: cells one under another at full width, so positions and text hold.
                    let first = self.out.len();
                    let mut at = pos + 1;
                    for row in &t.rows {
                        at += 1;
                        for cell in &row.cells {
                            self.blocks(&cell.blocks, Context::Cell { header: cell.header }, x, width, at + 1, None)?;
                            at += 2 + cell.blocks.iter().map(Block::size).sum::<u32>();
                        }
                        at += 1;
                    }
                    self.pad(first);
                }
            }
            marker = None;
            pos += b.size();
        }
        Some(())
    }

    /// Add the block gap before the first and after the last item produced since `first`.
    fn pad(&mut self, first: usize) {
        let set = |item: Option<&mut FlowItem>, before: bool| match item {
            Some(FlowItem::Para(p)) if before => p.space_before += BLOCK_GAP,
            Some(FlowItem::Para(p)) => p.space_after += BLOCK_GAP,
            Some(FlowItem::Box(b)) if before => b.space_before += BLOCK_GAP,
            Some(FlowItem::Box(b)) => b.space_after += BLOCK_GAP,
            _ => {}
        };
        if self.out.len() > first {
            set(self.out.get_mut(first), true);
            set(self.out.last_mut(), false);
        }
    }
}

/// Flow items of one top-level block for a text area `width` wide.
pub fn flatten(block: &Block, width: f32, footnote_base: u32, sh: &Shapers<'_>) -> Option<BlockFlow> {
    let mut f = Flattener { sh, out: Vec::new(), footnote_base, footnotes: 0 };
    f.blocks(std::slice::from_ref(block), Context::Body, 0.0, width, 0, None)?;
    Some(BlockFlow { items: f.out, footnotes: f.footnotes, footnote_base, width })
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlacedBox {
    pub rect: Rect,
    pub pos: u32,
    pub kind: LeafKind,
    pub data: Option<String>,
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
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageLayout {
    /// Lines and boxes in document order.
    pub entries: Vec<Entry>,
    /// Header and footer lines: drawn, but not part of the document's positions.
    pub running: Vec<PlacedLine>,
}

struct Paginator<'a> {
    page: &'a PageSetup,
    pages: Vec<PageLayout>,
    y: f32,
    /// Space owed after the previous item; dropped at a page break.
    pending: f32,
}

impl Paginator<'_> {
    fn top(&self) -> f32 {
        self.page.margin_top
    }
    fn bottom(&self) -> f32 {
        (self.page.height - self.page.margin_bottom).max(self.top() + 1.0)
    }
    fn at_top(&self) -> bool {
        self.pages.last().is_none_or(|p| p.entries.is_empty())
    }
    fn new_page(&mut self) {
        self.pages.push(PageLayout::default());
        self.y = self.top();
        self.pending = 0.0;
    }
    fn push(&mut self, e: Entry) {
        if let Some(p) = self.pages.last_mut() {
            p.entries.push(e);
        }
    }
    /// Gap before an item with `space_before`: nothing at the top of a page.
    fn gap(&self, space_before: f32) -> f32 {
        if self.at_top() { 0.0 } else { self.pending + space_before }
    }
}

/// Height an item needs so that a "keep with next" item before it is not left alone at the bottom of a page.
fn first_chunk(item: &FlowItem) -> f32 {
    match item {
        FlowItem::Para(p) => {
            let n = if p.keep_lines { p.lines.len() } else { p.lines.len().min(2) };
            p.space_before + p.lines.iter().take(n).map(|l| l.height).sum::<f32>()
        }
        FlowItem::Box(b) => b.space_before + b.h,
        FlowItem::PageBreak { .. } => 0.0,
    }
}

fn full_height(item: &FlowItem) -> f32 {
    match item {
        FlowItem::Para(p) => p.space_before + p.lines.iter().map(|l| l.height).sum::<f32>() + p.space_after,
        FlowItem::Box(b) => b.space_before + b.h + b.space_after,
        FlowItem::PageBreak { .. } => 0.0,
    }
}

/// Stack flow items onto pages. `flows` are the top-level blocks in order with their start positions.
pub fn paginate(page: &PageSetup, flows: &[(u32, Arc<BlockFlow>)]) -> Vec<PageLayout> {
    let mut pg = Paginator { page, pages: Vec::new(), y: 0.0, pending: 0.0 };
    pg.new_page();
    let left = page.margin_left;
    let items: Vec<(u32, &FlowItem)> =
        flows.iter().flat_map(|(start, f)| f.items.iter().map(move |i| (*start, i))).collect();
    let page_h = pg.bottom() - pg.top();

    for (idx, (start, item)) in items.iter().enumerate() {
        // Keep with next: this item and everything chained to it must be followed, on the same page, by the
        // beginning of the first item that is not chained.
        let keeps = |i: &FlowItem| matches!(i, FlowItem::Para(p) if p.keep_next);
        if keeps(item) && !pg.at_top() {
            let mut need = pg.pending;
            let mut j = idx;
            while let Some((_, it)) = items.get(j) {
                if keeps(it) && j - idx < 4 {
                    need += full_height(it);
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
        match item {
            FlowItem::PageBreak { pos } => {
                // The break itself is a zero-height box where it happens, so a caret can stand before it.
                let y = pg.y + pg.pending;
                pg.push(Entry::Box(PlacedBox {
                    rect: Rect::new(left, y.min(pg.bottom()), 0.0, 0.0),
                    pos: start + pos,
                    kind: LeafKind::PageBreak,
                    data: None,
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
                }));
                pg.y = y + h;
                pg.pending = b.space_after;
            }
            FlowItem::Para(p) => {
                if p.page_break_before && !pg.at_top() {
                    pg.new_page();
                }
                let mut next = 0usize;
                let n = p.lines.len();
                while next < n {
                    let gap = if next == 0 { pg.gap(p.space_before) } else { 0.0 };
                    let room = pg.bottom() - pg.y - gap + 0.01;
                    let mut fit = 0usize;
                    let mut used = 0.0f32;
                    for l in p.lines.iter().skip(next) {
                        if used + l.height > room {
                            break;
                        }
                        used += l.height;
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
                    for l in p.lines.iter().skip(next).take(fit) {
                        pg.push(Entry::Line(PlacedLine {
                            line: Arc::clone(l),
                            x: left + p.x,
                            y,
                            box_w: p.width,
                            base: start + p.pos,
                            background: p.background,
                        }));
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

/// Add header and footer lines to every page (not to a title page).
pub fn add_running(pages: &mut [PageLayout], page: &PageSetup, running: &Running, sh: &Shapers<'_>) -> Option<()> {
    let width = (page.width - page.margin_left - page.margin_right).max(1.0);
    let line = |text: &str, size: f32, align: Align| -> Option<Vec<Line>> {
        let para = Paragraph {
            inlines: vec![sfdoc_model::Inline::Text(sfdoc_model::Text {
                text: text.to_owned(),
                marks: Default::default(),
            })],
            ..Default::default()
        };
        let style = ParaStyle {
            align,
            line_mult: 1.0,
            left: 0.0,
            right: 0.0,
            first: 0.0,
            space_before: 0.0,
            space_after: 0.0,
            keep_next: false,
            keep_lines: false,
            page_break_before: false,
            code: false,
            text: TextDefaults { family: crate::style::BODY_FONT, size, bold: false, italic: false },
        };
        layout_paragraph(&ParaInput { para: &para, style: &style, width, marker: None, footnote_base: 0 }, sh)
    };
    for (i, p) in pages.iter_mut().enumerate() {
        if i == 0 && running.title_page {
            continue;
        }
        let number = (i + 1).to_string();
        let place = |lines: Vec<Line>, y: f32, p: &mut PageLayout| -> f32 {
            let mut y = y;
            for l in lines {
                let h = l.height;
                p.running.push(PlacedLine {
                    line: Arc::new(l),
                    x: page.margin_left,
                    y,
                    box_w: width,
                    base: 0,
                    background: None,
                });
                y += h;
            }
            y
        };
        if !running.header.is_empty() {
            place(line(&running.header, 12.0, Align::Left)?, RUNNING_EDGE, p);
        }
        if running.page_number == PageNumber::TopRight {
            place(line(&number, 14.0, Align::Right)?, RUNNING_EDGE, p);
        }
        // The footer grows upwards from its bottom edge: text first, the number under it.
        let mut footer: Vec<Line> = Vec::new();
        if !running.footer.is_empty() {
            footer.extend(line(&running.footer, 12.0, Align::Left)?);
        }
        if running.page_number == PageNumber::BottomCenter {
            footer.extend(line(&number, 14.0, Align::Center)?);
        }
        let h: f32 = footer.iter().map(|l| l.height).sum();
        place(footer, page.height - RUNNING_EDGE - h, p);
    }
    Some(())
}
