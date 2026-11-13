//! Walking the document by flat positions (see the crate docs for the scheme).

use crate::{Atom, Block, Document, Inline, Leaf, Paragraph, Text, utf16_len, utf16_slice};

/// Something met while walking the document in position order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event<'a> {
    /// A paragraph or heading; `pos` is the position before it, its content starts at `pos + 1`.
    Textblock {
        pos: u32,
        size: u32,
        para: &'a Paragraph,
    },
    /// A run of text starting at `pos`.
    Text {
        pos: u32,
        text: &'a Text,
    },
    Atom {
        pos: u32,
        atom: &'a Atom,
    },
    LeafBlock {
        pos: u32,
        leaf: &'a Leaf,
    },
}

/// Where a position falls among the top-level blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Loc {
    /// Index of the top-level block that contains the position, or `blocks.len()` at the very end.
    pub block: usize,
    /// Position of the start of that block.
    pub block_start: u32,
}

impl Inline {
    pub fn size(&self) -> u32 {
        match self {
            Inline::Text(t) => utf16_len(&t.text),
            Inline::Atom(_) => 1,
        }
    }
}

impl Paragraph {
    /// Size of the inline content (the node itself is 2 larger).
    pub fn content_size(&self) -> u32 {
        self.inlines.iter().map(Inline::size).sum()
    }
}

fn blocks_size(blocks: &[Block]) -> u32 {
    blocks.iter().map(Block::size).sum()
}

impl Block {
    /// Number of positions the block spans, its own open and close tokens included.
    pub fn size(&self) -> u32 {
        match self {
            Block::Paragraph(p) => 2 + p.content_size(),
            Block::List(l) => 2 + l.items.iter().map(|i| 2 + blocks_size(&i.blocks)).sum::<u32>(),
            Block::Quote(q) => 2 + blocks_size(&q.blocks),
            Block::Table(t) => {
                2 + t
                    .rows
                    .iter()
                    .map(|r| 2 + r.cells.iter().map(|c| 2 + blocks_size(&c.blocks)).sum::<u32>())
                    .sum::<u32>()
            }
            Block::Leaf(_) => 1,
        }
    }
}

fn walk<'a>(blocks: &'a [Block], mut pos: u32, f: &mut dyn FnMut(Event<'a>)) {
    for b in blocks {
        match b {
            Block::Paragraph(p) => {
                f(Event::Textblock { pos, size: 2 + p.content_size(), para: p });
                let mut at = pos + 1;
                for i in &p.inlines {
                    match i {
                        Inline::Text(t) => f(Event::Text { pos: at, text: t }),
                        Inline::Atom(a) => f(Event::Atom { pos: at, atom: a }),
                    }
                    at += i.size();
                }
            }
            Block::List(l) => {
                let mut at = pos + 1;
                for item in &l.items {
                    walk(&item.blocks, at + 1, f);
                    at += 2 + blocks_size(&item.blocks);
                }
            }
            Block::Quote(q) => walk(&q.blocks, pos + 1, f),
            Block::Table(t) => {
                let mut at = pos + 1;
                for row in &t.rows {
                    at += 1;
                    for cell in &row.cells {
                        walk(&cell.blocks, at + 1, f);
                        at += 2 + blocks_size(&cell.blocks);
                    }
                    at += 1;
                }
            }
            Block::Leaf(l) => f(Event::LeafBlock { pos, leaf: l }),
        }
        pos += b.size();
    }
}

impl Document {
    /// Size of the document content: the last valid position.
    pub fn content_size(&self) -> u32 {
        blocks_size(&self.blocks)
    }

    /// Visit every text block, text run, inline atom and leaf block in position order.
    pub fn walk<'a>(&'a self, mut f: impl FnMut(Event<'a>)) {
        walk(&self.blocks, 0, &mut f);
    }

    /// The top-level block containing `pos` (a position between two blocks belongs to the later one).
    pub fn locate(&self, pos: u32) -> Loc {
        let mut start = 0u32;
        for (i, b) in self.blocks.iter().enumerate() {
            let end = start + b.size();
            if pos < end {
                return Loc { block: i, block_start: start };
            }
            start = end;
        }
        Loc { block: self.blocks.len(), block_start: start }
    }

    /// The text between two positions. `block_separator` is put before every text block after the first one
    /// touched; atoms and leaf blocks contribute nothing.
    pub fn text_between(&self, from: u32, to: u32, block_separator: &str) -> String {
        let mut out = String::new();
        let mut first = true;
        self.walk(|e| match e {
            Event::Textblock { pos, size, .. } => {
                if pos < to && pos + size > from {
                    if first {
                        first = false;
                    } else {
                        out.push_str(block_separator);
                    }
                }
            }
            Event::Text { pos, text } => {
                let len = utf16_len(&text.text);
                if pos < to && pos + len > from {
                    out.push_str(utf16_slice(&text.text, from.saturating_sub(pos), to - pos));
                }
            }
            Event::Atom { .. } | Event::LeafBlock { .. } => {}
        });
        out
    }
}
