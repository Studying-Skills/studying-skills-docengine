//! Where things are: the questions an editor asks a layout (caret, click, selection, line movement).

use sfdoc_geom::Rect;

use crate::Layout;
use crate::flow::{Entry, PlacedLine};

/// Width added to a selected line's end when the selection goes on to the next line (the "newline" sliver).
const NEWLINE_SLIVER: f32 = 4.0;

/// Where the caret for a position is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caret {
    pub page: usize,
    pub x: f32,
    pub y: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageRect {
    pub page: usize,
    pub rect: Rect,
}

/// Page x of the caret at paragraph offset `off` on a placed line.
fn line_x(l: &PlacedLine, off: u32) -> f32 {
    let line = &l.line;
    let mut x = l.x + line.x;
    for f in &line.frags {
        if off < f.from {
            break;
        }
        let i = (off - f.from).min(f.to - f.from) as usize;
        x = l.x + f.x + f.carets.get(i).copied().unwrap_or(0.0);
        if off < f.to {
            break;
        }
    }
    x
}

/// Paragraph offset on a placed line nearest to page x.
fn line_offset_at(l: &PlacedLine, x: f32) -> u32 {
    let line = &l.line;
    let mut best = (f32::MAX, line.from);
    for f in &line.frags {
        for (i, c) in f.carets.iter().enumerate() {
            let d = (l.x + f.x + c - x).abs();
            // Strictly nearer only: of two positions with one caret (inside a surrogate pair) the first wins.
            if d < best.0 {
                best = (d, f.from + i as u32);
            }
        }
    }
    // A soft-wrapped line's last position displays at the start of the next line: stop before the trailing space.
    best.1.min(line_end(l) - l.base)
}

/// Last position of the line a caret can show on this line.
fn line_end(l: &PlacedLine) -> u32 {
    let line = &l.line;
    if line.hard || line.to == line.from {
        return l.base + line.to;
    }
    let trailing = line.frags.last().map_or(0, |f| {
        let n = f.carets.len();
        // Trailing spaces are the positions after the last ink: count carets beyond the first of them.
        match &f.kind {
            crate::para::FragKind::Text(t) => {
                let spaces = t.text.len() - t.text.trim_end_matches(' ').len();
                spaces.min(n.saturating_sub(1)) as u32
            }
            _ => 0,
        }
    });
    (l.base + line.to - trailing.max(1)).max(l.base + line.from)
}

impl Layout {
    fn entries(&self) -> impl Iterator<Item = (usize, usize, &Entry)> {
        self.pages.iter().enumerate().flat_map(|(pi, p)| p.entries.iter().enumerate().map(move |(ei, e)| (pi, ei, e)))
    }

    /// The entry a caret at `pos` belongs to: the last one that contains it (a position shared by the end of one
    /// wrapped line and the start of the next shows on the next), else the first one after it, else the last.
    fn locate(&self, pos: u32) -> Option<(usize, usize, &Entry)> {
        let mut found = None;
        let mut after = None;
        let mut last = None;
        for (pi, ei, e) in self.entries() {
            let (from, to) = e.span();
            if from <= pos && pos <= to {
                found = Some((pi, ei, e));
            } else if from > pos && after.is_none() {
                after = Some((pi, ei, e));
            }
            last = Some((pi, ei, e));
            if from > pos && found.is_some() {
                break;
            }
        }
        found.or(after).or(last)
    }

    pub fn caret_rect(&self, pos: u32) -> Option<Caret> {
        let (page, _, e) = self.locate(pos)?;
        Some(match e {
            Entry::Line(l) => {
                let off = pos.saturating_sub(l.base).clamp(l.line.from, l.line.to);
                Caret { page, x: line_x(l, off), y: l.y, height: l.line.baseline + l.line.descent }
            }
            Entry::Box(b) => Caret {
                page,
                x: if pos > b.pos { b.rect.right() } else { b.rect.x },
                y: b.rect.y,
                height: b.rect.h.max(1.0),
            },
        })
    }

    pub fn page_of(&self, pos: u32) -> Option<usize> {
        self.locate(pos).map(|(p, _, _)| p)
    }

    /// The position nearest to a point of a page.
    pub fn hit_test(&self, page: usize, x: f32, y: f32) -> Option<u32> {
        let p = self.pages.get(page.min(self.pages.len().saturating_sub(1)))?;
        // Nearest entry: vertical distance first, then horizontal distance to its box (table cells sit side by
        // side at one height). Of equally near entries the first in document order wins.
        let mut pick: Option<(&Entry, (f32, f32))> = None;
        for e in &p.entries {
            let dy = if y < e.top() {
                e.top() - y
            } else if y >= e.bottom() {
                y - e.bottom() + f32::MIN_POSITIVE
            } else {
                0.0
            };
            let (left, right) = match e {
                Entry::Line(l) => (l.x, l.x + l.box_w),
                Entry::Box(b) => (b.rect.x, b.rect.right()),
            };
            let dx = if x < left {
                left - x
            } else if x > right {
                x - right
            } else {
                0.0
            };
            if pick.is_none_or(|(_, best)| (dy, dx) < best) {
                pick = Some((e, (dy, dx)));
            }
        }
        Some(match pick?.0 {
            Entry::Line(l) => l.base + line_offset_at(l, x),
            Entry::Box(b) => {
                if x > b.rect.x + b.rect.w / 2.0 {
                    b.pos + 1
                } else {
                    b.pos
                }
            }
        })
    }

    /// Rectangles covering the selection `from..to`, line by line.
    pub fn selection_rects(&self, from: u32, to: u32) -> Vec<PageRect> {
        let (from, to) = (from.min(to), from.max(to));
        let mut out = Vec::new();
        if from == to {
            return out;
        }
        for (page, _, e) in self.entries() {
            let (start, end) = e.span();
            if end < from || start >= to {
                continue;
            }
            match e {
                Entry::Line(l) => {
                    let a = from.max(start) - l.base;
                    let b = to.min(end) - l.base;
                    let x1 = line_x(l, a);
                    let mut x2 = line_x(l, b);
                    if to > end {
                        x2 += NEWLINE_SLIVER;
                    }
                    if x2 > x1 {
                        out.push(PageRect { page, rect: Rect::new(x1, l.y, x2 - x1, l.line.height) });
                    }
                }
                Entry::Box(b) => {
                    if from <= start && to >= end && b.rect.w > 0.0 {
                        out.push(PageRect { page, rect: b.rect });
                    }
                }
            }
        }
        out
    }

    /// First and last position of the line `pos` is on (Home and End).
    pub fn line_bounds(&self, pos: u32) -> Option<(u32, u32)> {
        let (_, _, e) = self.locate(pos)?;
        Some(match e {
            Entry::Line(l) => (l.base + l.line.from, line_end(l)),
            Entry::Box(b) => (b.pos, b.pos + 1),
        })
    }

    /// The position one line up (`down == false`) or down from `pos`, as close to page x `goal_x` as that line
    /// allows. At the first or last line the position does not change.
    pub fn line_move(&self, pos: u32, down: bool, goal_x: f32) -> Option<u32> {
        let (pi, ei, _) = self.locate(pos)?;
        let flat: Vec<(usize, usize, &Entry)> = self.entries().collect();
        let here = flat.iter().position(|(p, e, _)| *p == pi && *e == ei)?;
        let Some(target) = (if down { Some(here + 1) } else { here.checked_sub(1) }) else { return Some(pos) };
        Some(match flat.get(target) {
            Some((_, _, Entry::Line(l))) => l.base + line_offset_at(l, goal_x),
            Some((_, _, Entry::Box(b))) => b.pos,
            None => pos,
        })
    }
}
