//! `sfdoc bench`: how long the layout takes on a report of a given length — the full layout, and what one
//! keystroke costs (replace a paragraph, lay out again, redraw the page it is on).

use std::time::{Duration, Instant};

use sfdoc_fonts::FontSet;
use sfdoc_layout::Layouter;
use sfdoc_model::{Block, Document};

const SENTENCE: &str = "У роботі розглянуто три алгоритми: сортування бульбашкою, сортування вставками та швидке сортування. \
Для кожного з них виміряно час виконання на масивах із тисячі, десяти тисяч і ста тисяч елементів. ";

fn paragraph(i: usize, extra: &str) -> serde_json::Value {
    let mut inlines =
        vec![serde_json::json!({"type": "text", "text": format!("{SENTENCE}{SENTENCE}Абзац {i}.{extra}")})];
    if i.is_multiple_of(9) {
        inlines.push(
            serde_json::json!({"type": "atom", "kind": "footnote", "data": "Кнут Д. Мистецтво програмування. Т. 3."}),
        );
    }
    serde_json::json!({"type": "paragraph", "align": "justify", "inlines": inlines})
}

/// A report shaped like a real one: contents, sections with justified paragraphs, footnotes, a table and a list
/// per section. About seven paragraphs make a page.
fn document(pages: usize) -> Result<Document, String> {
    let cell = |t: &str, header: bool| serde_json::json!({"header": header, "blocks": [{"type": "paragraph", "inlines": [{"type": "text", "text": t}]}]});
    let mut blocks = vec![serde_json::json!({"type": "leaf", "kind": "toc", "data": "Зміст"})];
    let mut n = 0usize;
    for section in 0..pages.div_ceil(4).max(1) {
        blocks.push(serde_json::json!({"type": "paragraph", "level": 1, "inlines": [{"type": "text", "text": format!("Розділ {}", section + 1)}]}));
        for _ in 0..24 {
            n += 1;
            blocks.push(paragraph(n, ""));
        }
        let rows: Vec<_> = (0..6).map(|r| serde_json::json!({"cells": [cell("Алгоритм", r == 0), cell("квадратична складність", r == 0), cell("1250", r == 0)]})).collect();
        blocks.push(serde_json::json!({"type": "table", "rows": rows}));
        let items: Vec<_> = (0..4).map(|i| serde_json::json!({"blocks": [{"type": "paragraph", "inlines": [{"type": "text", "text": format!("пункт {i}")}]}]})).collect();
        blocks.push(serde_json::json!({"type": "list", "items": items}));
    }
    serde_json::from_value(serde_json::json!({"version": sfdoc_model::FORMAT_VERSION, "blocks": blocks}))
        .map_err(|e| e.to_string())
}

pub struct Report {
    pub pages: usize,
    pub blocks: usize,
    pub full: Duration,
    /// One keystroke each: sorted, shortest first.
    pub edits: Vec<Duration>,
}

impl Report {
    pub fn median(&self) -> Duration {
        self.edits.get(self.edits.len() / 2).copied().unwrap_or_default()
    }
    pub fn worst(&self) -> Duration {
        self.edits.last().copied().unwrap_or_default()
    }
}

pub fn bench(pages: usize, fonts: &FontSet) -> Result<Report, String> {
    let mut doc = document(pages)?;
    let mut layouter = Layouter::new();
    let start = Instant::now();
    let layout = layouter.layout(&doc, fonts).map_err(|e| e.to_string())?;
    let full = start.elapsed();
    let page_count = layout.pages.len();

    // Type into paragraphs spread over the document: the first pages cost the most (everything after them moves).
    let targets: Vec<usize> = doc
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| matches!(b, Block::Paragraph(p) if p.level == 0))
        .map(|(i, _)| i)
        .collect();
    let mut edits = Vec::new();
    for k in 0..40usize {
        let Some(&at) = targets.get(k * targets.len().saturating_sub(1) / 39) else { break };
        let block: Block = serde_json::from_value(paragraph(at, &" я".repeat(k % 5 + 1))).map_err(|e| e.to_string())?;
        let start = Instant::now();
        doc.replace_blocks(at, at + 1, vec![block]).map_err(|e| e.to_string())?;
        layouter.replaced(at, at + 1, 1);
        let layout = layouter.layout(&doc, fonts).map_err(|e| e.to_string())?;
        // What the editor does next: compare page fingerprints, fetch the display list of the page with the caret.
        let prints: u64 = (0..layout.pages.len()).filter_map(|i| layout.fingerprint(i)).fold(0, u64::wrapping_add);
        let pos: u32 = doc.blocks.iter().take(at).map(Block::size).sum::<u32>() + 1;
        let page = layout.page_of(pos).unwrap_or(0);
        let list = layout.page(page, fonts).ok_or("the page is gone")?;
        std::hint::black_box((prints, list));
        edits.push(start.elapsed());
    }
    edits.sort();
    Ok(Report { pages: page_count, blocks: doc.blocks.len(), full, edits })
}

pub fn run(pages: usize, budget_ms: Option<f64>, fonts: &FontSet) -> Result<String, String> {
    let r = bench(pages, fonts)?;
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let out = format!(
        "{} pages, {} blocks\nfull layout: {:.1} ms\none keystroke (layout + fingerprints + display list of its page): median {:.2} ms, worst {:.2} ms of {}\n",
        r.pages,
        r.blocks,
        ms(r.full),
        ms(r.median()),
        ms(r.worst()),
        r.edits.len()
    );
    match budget_ms {
        Some(b) if ms(r.median()) > b => Err(format!("{out}over budget: the median keystroke takes more than {b} ms")),
        _ => Ok(out),
    }
}
