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
use sfdoc_model::{Align, Block, Color, Leaf, LeafKind, List, PageNumber, PageSetup, Paragraph, Running, Table};

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
    Table(TableFlow),
    PageBreak { pos: u32 },
}

/// Cell padding left and right (Word's default 0.19 cm) and above and below the content.
pub const CELL_PAD_X: f32 = 5.4;
pub const CELL_PAD_Y: f32 = 2.0;
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
    let mut pending = 0.0;
    let mut first = true;
    for item in items {
        let (before, body, after) = match item {
            FlowItem::Para(p) => (p.space_before, p.lines.iter().map(|l| l.height).sum::<f32>(), p.space_after),
            FlowItem::Box(b) => (b.space_before, b.h, b.space_after),
            FlowItem::Table(t) => (t.space_before, t.height(), t.space_after),
            FlowItem::PageBreak { .. } => continue,
        };
        h += if first { 0.0 } else { pending + before } + body;
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
                Block::Table(t) => self.table(t, x, width, pos)?,
            }
            marker = None;
            pos += b.size();
        }
        Some(())
    }

    /// Width the blocks would take on one line each: what "size the columns by content" measures.
    fn natural_width(&self, blocks: &[Block], ctx: Context) -> Option<f32> {
        let mut w = 0.0f32;
        for b in blocks {
            let own = match b {
                Block::Paragraph(p) => {
                    let style = para_style(p, ctx);
                    let inp = ParaInput { para: p, style: &style, width: 1.0e6, marker: None, footnote_base: 0 };
                    let lines = layout_paragraph(&inp, self.sh)?;
                    style.left + style.right + style.first.max(0.0) + lines.iter().map(|l| l.width).fold(0.0, f32::max)
                }
                Block::Leaf(l) => l.width.max(0.0),
                Block::List(list) => {
                    let mut m = 0.0f32;
                    for item in &list.items {
                        m = m.max(self.natural_width(&item.blocks, Context::ListItem)?);
                    }
                    m + LIST_INDENT
                }
                Block::Quote(q) => self.natural_width(&q.blocks, Context::Quote)? + cm(1.25),
                Block::Table(t) => {
                    let mut m = 0.0f32;
                    for row in &t.rows {
                        let mut sum = 0.0;
                        for cell in &row.cells {
                            sum += self.natural_width(&cell.blocks, Context::Cell { header: cell.header })?
                                + 2.0 * CELL_PAD_X;
                        }
                        m = m.max(sum);
                    }
                    m
                }
            };
            w = w.max(own);
        }
        Some(w)
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
                let mut natural = vec![MIN_COLUMN; ncols];
                for s in slots.iter().filter(|s| s.colspan == 1) {
                    let w = self.natural_width(&s.cell.blocks, Context::Cell { header: s.cell.header })?
                        + 2.0 * CELL_PAD_X
                        + 1.0;
                    if let Some(n) = natural.get_mut(s.col) {
                        *n = n.max(w.min(width));
                    }
                }
                let sum: f32 = natural.iter().sum();
                if sum <= width {
                    // Room to spare: the table fills the text area, columns keep their proportions.
                    natural.iter().map(|n| n * width / sum).collect()
                } else {
                    natural.iter().map(|n| (n * width / sum).max(MIN_COLUMN)).collect()
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
            let height = stack_height(&items) + 2.0 * CELL_PAD_Y;
            if let Some(row) = rows.get_mut(s.row) {
                row.cells.push(CellFlow {
                    col: s.col,
                    colspan: s.colspan,
                    rowspan: s.rowspan,
                    items,
                    height,
                    background: s.cell.background,
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
                row.height = base + 2.0 * CELL_PAD_Y;
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
    /// Table cells on this page (entries point here through `region`); a nested table's cells come after the
    /// cell that holds it.
    pub cells: Vec<Rect>,
    /// Filled rectangles under the text: cell shading and the table grid.
    pub decor: Vec<(Rect, Color)>,
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
    while let Some(item) = items.get(cur.item) {
        let gap = |before: f32, cur: &Cursor| if cur.started { cur.pending + before } else { 0.0 };
        match item {
            FlowItem::PageBreak { .. } => cur.item += 1,
            FlowItem::Para(p) => {
                while let Some(l) = p.lines.get(cur.line) {
                    let g = if cur.line == 0 { gap(p.space_before, cur) } else { 0.0 };
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
                    }
                    used += g + l.height;
                    cur.line += 1;
                    cur.started = true;
                    cur.pending = 0.0;
                    force = false;
                }
                cur.item += 1;
                cur.line = 0;
                cur.pending = p.space_after;
            }
            FlowItem::Box(b) => {
                let g = gap(b.space_before, cur);
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
                }
                used += g + h;
                cur.item += 1;
                cur.started = true;
                cur.pending = b.space_after;
                force = false;
            }
            FlowItem::Table(t) => {
                // A table inside a cell is never split.
                let g = gap(t.space_before, cur);
                let h = t.height();
                if used + g + h > room + 0.01 && !force {
                    return used;
                }
                place_rows(t, 0, t.rows.len(), start, left, y + used + g, ghost, page);
                used += g + h;
                cur.item += 1;
                cur.started = true;
                cur.pending = t.space_after;
                force = false;
            }
        }
    }
    used
}

/// The frame and shading of one cell.
fn cell_decor(rect: Rect, background: Option<Color>, page: &mut PageLayout) {
    if let Some(bg) = background {
        page.decor.push((rect, bg));
    }
    let b = TABLE_BORDER;
    for r in [
        Rect::new(rect.x - b / 2.0, rect.y - b / 2.0, rect.w + b, b),
        Rect::new(rect.x - b / 2.0, rect.bottom() - b / 2.0, rect.w + b, b),
        Rect::new(rect.x - b / 2.0, rect.y - b / 2.0, b, rect.h + b),
        Rect::new(rect.right() - b / 2.0, rect.y - b / 2.0, b, rect.h + b),
    ] {
        page.decor.push((r, Color::BLACK));
    }
}

fn cell_rect(t: &TableFlow, c: &CellFlow, left: f32, y: f32, h: f32) -> Rect {
    let x0 = t.cols.get(c.col).copied().unwrap_or(0.0);
    let x1 = t.cols.get(c.col + c.colspan).copied().or(t.cols.last().copied()).unwrap_or(x0);
    Rect::new(left + t.x + x0, y, (x1 - x0).max(0.0), h)
}

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
            cell_decor(rect, c.background, page);
            let region = open_region(rect, ghost, page);
            place_some(
                &c.items,
                &mut Cursor::default(),
                start,
                left,
                top + CELL_PAD_Y,
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
        // The header rows and the first row under them.
        FlowItem::Table(t) => t.space_before + t.rows_height(0, t.group_end(t.header_rows)),
        FlowItem::PageBreak { .. } => 0.0,
    }
}

fn full_height(item: &FlowItem) -> f32 {
    match item {
        FlowItem::Para(p) => p.space_before + p.lines.iter().map(|l| l.height).sum::<f32>() + p.space_after,
        FlowItem::Box(b) => b.space_before + b.h + b.space_after,
        FlowItem::Table(t) => t.space_before + t.height() + t.space_after,
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
                    let fresh_room = page_h - if r >= t.header_rows { header_h } else { 0.0 };
                    if pg.y + gap + h <= pg.bottom() + 0.01 {
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
                            let room = pg.bottom() - y - 2.0 * CELL_PAD_Y;
                            // Too little room to start the row here: start on the next page.
                            if first && room < 2.0 * CELL_PAD_Y + 20.0 && pg.y > pg.top() + 0.01 {
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
                                        y + CELL_PAD_Y,
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
                                seg += 2.0 * CELL_PAD_Y;
                                if unfinished {
                                    seg = seg.max(bottom - y);
                                }
                                for (c, region) in row.cells.iter().zip(regions) {
                                    let rect = cell_rect(t, c, left, y, seg);
                                    cell_decor(rect, c.background, page);
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
                            region: 0,
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
                    region: 0,
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
