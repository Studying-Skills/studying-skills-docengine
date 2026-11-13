use std::sync::Arc;

use proptest::prelude::*;
use sfdoc_fonts::{Font, FontSet};
use sfdoc_geom::mm;
use sfdoc_model::*;

use super::*;
use crate::para::FragKind;

fn fonts() -> FontSet {
    let mut set = FontSet::new();
    for name in ["Serif-Regular", "Serif-Bold", "Serif-Italic", "Serif-BoldItalic", "Sans-Regular", "Mono-Regular"] {
        let path = format!("{}/../../assets/fonts/liberation/Liberation{name}.ttf", env!("CARGO_MANIFEST_DIR"));
        set.add_described(Font::from_bytes(std::fs::read(path).unwrap()).unwrap()).unwrap();
    }
    set
}

fn t(s: &str) -> Inline {
    Inline::Text(Text { text: s.into(), marks: Marks::default() })
}
fn tm(s: &str, marks: Marks) -> Inline {
    Inline::Text(Text { text: s.into(), marks })
}
fn para_of(inlines: Vec<Inline>) -> Block {
    Block::Paragraph(Paragraph { inlines, ..Default::default() })
}
fn para(s: &str) -> Block {
    para_of(if s.is_empty() { vec![] } else { vec![t(s)] })
}
fn heading(level: u8, s: &str) -> Block {
    Block::Paragraph(Paragraph { level, inlines: vec![t(s)], ..Default::default() })
}
fn leaf(kind: LeafKind, width: f32, height: f32) -> Block {
    Block::Leaf(Leaf { kind, width, height, data: None })
}
fn doc(blocks: Vec<Block>) -> Document {
    Document { blocks, ..Default::default() }
}
fn lay(d: &Document) -> Layout {
    layout(d, &fonts()).unwrap()
}
fn lines(l: &Layout) -> Vec<(usize, &PlacedLine)> {
    l.pages
        .iter()
        .enumerate()
        .flat_map(|(i, p)| {
            p.entries.iter().filter_map(move |e| if let Entry::Line(l) = e { Some((i, l)) } else { None })
        })
        .collect()
}
fn line_text(l: &PlacedLine) -> String {
    l.line
        .frags
        .iter()
        .filter_map(|f| if let FragKind::Text(t) = &f.kind { Some(t.text.as_str()) } else { None })
        .collect()
}
const LEFT: f32 = 85.039_37; // 30 mm
const TEXT_W: f32 = 481.889_77; // 170 mm
const INDENT: f32 = 35.433_07; // 1.25 cm

const LOREM: &str = "У роботі розглянуто три алгоритми: сортування бульбашкою, сортування вставками та швидке сортування. Для кожного з них виміряно час виконання на масивах із тисячі, десяти тисяч і ста тисяч елементів; результати наведено в таблиці. Як видно з вимірювань, швидке сортування випереджає два інші алгоритми вже на масиві з десяти тисяч елементів, і це узгоджується з теоретичною оцінкою.";

#[test]
fn an_empty_document_is_one_empty_a4_page() {
    let l = lay(&Document::default());
    assert_eq!(l.pages.len(), 1);
    assert!(l.pages[0].entries.is_empty());
    assert_eq!(l.caret_rect(0), None);
    assert!((l.page_size().w - 595.2756).abs() < 0.001);
}

#[test]
fn layout_refuses_invalid_input_instead_of_panicking() {
    assert_eq!(layout(&Document::default(), &FontSet::new()), Err(LayoutError::NoFonts));
    let bad = Document { version: 7, ..Default::default() };
    assert_eq!(layout(&bad, &fonts()), Err(LayoutError::Model(ModelError::UnsupportedVersion(7))));
}

#[test]
fn a_body_paragraph_starts_at_the_margin_plus_the_first_line_indent() {
    let l = lay(&doc(vec![para("Привіт")]));
    let ls = lines(&l);
    assert_eq!(ls.len(), 1);
    let line = ls[0].1;
    assert!((line.x - LEFT).abs() < 0.01 && (line.y - mm(20.0)).abs() < 0.01);
    assert!((line.line.x - INDENT).abs() < 0.01);
    // 14 pt Times: single line 16.1 pt, one and a half 24.15 pt; the extra space goes below the text.
    assert!((line.line.height - 24.149).abs() < 0.01, "{}", line.line.height);
    assert!((line.line.baseline - 14.0 * 1825.0 / 2048.0).abs() < 0.01);
    let width = fonts().get(sfdoc_fonts::FontId(0)).unwrap().shaper().unwrap().advance("Привіт", 14.0);
    let start = l.caret_rect(1).unwrap();
    let end = l.caret_rect(7).unwrap();
    assert!((start.x - (LEFT + INDENT)).abs() < 0.01);
    assert!((end.x - start.x - width).abs() < 0.01);
    assert_eq!((start.page, start.y), (0, line.y));
    // The caret spans ascent + descent, not the whole spaced line.
    assert!((start.height - 14.0 * (1825.0 + 443.0) / 2048.0).abs() < 0.01);
}

#[test]
fn long_text_wraps_and_justifies_every_line_but_the_last() {
    let l = lay(&doc(vec![para(LOREM)]));
    let ls = lines(&l);
    assert!(ls.len() >= 5, "{} lines", ls.len());
    for (i, (_, pl)) in ls.iter().enumerate() {
        let line = &pl.line;
        let first = i == 0;
        let room = TEXT_W - if first { INDENT } else { 0.0 };
        assert!(line.width <= room + 0.02, "line {i} is {} wide, room {room}", line.width);
        let last = line.frags.last().unwrap();
        let ink_right = last.x + if let FragKind::Text(t) = &last.kind { t.ink_width } else { last.advance };
        if i + 1 < ls.len() {
            assert!(!line.hard);
            assert!((ink_right - TEXT_W).abs() < 0.02, "line {i} ends at {ink_right}, the right edge is {TEXT_W}");
        } else {
            assert!(line.hard);
            assert!(ink_right < TEXT_W - 1.0);
        }
        // Lines stack without gaps and carry on where the previous one stopped.
        if i > 0 {
            let prev = ls[i - 1].1;
            assert!((pl.y - (prev.y + prev.line.height)).abs() < 0.01);
            assert_eq!(line.from, prev.line.to);
        }
    }
    assert_eq!(ls.iter().map(|(_, l)| line_text(l)).collect::<String>(), LOREM);
}

#[test]
fn alignment_moves_the_line() {
    let aligned = |a: Align| {
        let p = Block::Paragraph(Paragraph {
            props: ParaProps { align: Some(a), indent_first: Some(0.0), ..Default::default() },
            inlines: vec![t("Текст")],
            ..Default::default()
        });
        let l = lay(&doc(vec![p]));
        let (x, w) = (lines(&l)[0].1.line.x, lines(&l)[0].1.line.width);
        (x, w)
    };
    let (left, w) = aligned(Align::Left);
    assert!(left.abs() < 0.01);
    assert!((aligned(Align::Center).0 - (TEXT_W - w) / 2.0).abs() < 0.01);
    assert!((aligned(Align::Right).0 - (TEXT_W - w)).abs() < 0.01);
    assert!(aligned(Align::Justify).0.abs() < 0.01);
}

#[test]
fn a_click_on_a_caret_gives_its_position_back() {
    let d = doc(vec![heading(1, "Заголовок"), para(LOREM), para(""), para("Останній абзац.")]);
    let l = lay(&d);
    let mut checked = 0;
    d.walk(|e| {
        if let Event::Textblock { pos, size, .. } = e {
            for p in pos + 1..pos + size {
                let c = l.caret_rect(p).unwrap();
                assert_eq!(l.hit_test(c.page, c.x, c.y + c.height / 2.0), Some(p), "position {p}");
                checked += 1;
            }
        }
    });
    assert!(checked > 400);
    // Far outside the text: the nearest line still answers.
    assert_eq!(l.hit_test(0, -50.0, -50.0), Some(1));
    assert_eq!(l.hit_test(0, 5000.0, 5000.0), Some(d.content_size() - 1));
    assert_eq!(l.hit_test(99, 0.0, 0.0), Some(1));
}

#[test]
fn mixed_sizes_fonts_and_scripts_share_a_baseline() {
    let big = Marks { size: Some(24.0), ..Default::default() };
    let sup = Marks { vert: VertAlign::Sup, ..Default::default() };
    let sub = Marks { vert: VertAlign::Sub, ..Default::default() };
    let code = Marks { code: true, ..Default::default() };
    let bold = Marks { bold: true, italic: true, ..Default::default() };
    let l = lay(&doc(vec![para_of(vec![
        t("x"),
        tm("2", sup),
        t(" H"),
        tm("2", sub),
        t("O "),
        tm("Великий", big),
        t(" "),
        tm("код", code),
        t(" "),
        tm("жирний", bold),
    ])]));
    let line = &lines(&l)[0].1.line;
    // The tallest font sets the line: 24 pt Times, single 27.6 pt, times 1.5.
    assert!((line.height - 24.0 * 2355.0 / 2048.0 * 1.5).abs() < 0.01, "{}", line.height);
    assert!((line.baseline - 24.0 * 1825.0 / 2048.0).abs() < 0.01);
    let texts: Vec<_> =
        line.frags.iter().filter_map(|f| if let FragKind::Text(t) = &f.kind { Some(t) } else { None }).collect();
    let find = |s: &str| *texts.iter().find(|t| t.text.trim() == s).unwrap();
    assert!((find("2").size - 14.0 * 0.65).abs() < 0.01 && find("2").rise > 0.0);
    assert!(texts.iter().any(|t| t.text == "2" && t.rise < 0.0));
    assert_eq!(find("код").font, fonts().resolve(Some("Courier New"), false, false).unwrap().id);
    assert!((find("код").size - 12.0).abs() < 0.01);
    assert_eq!(find("жирний").font, fonts().resolve(Some("Times New Roman"), true, true).unwrap().id);
    // On the page: every glyph run of the line sits on the baseline, shifted only by its rise.
    let page = l.page(0, &fonts()).unwrap();
    let baseline = lines(&l)[0].1.y + line.baseline;
    let runs: Vec<_> = page.items.iter().filter_map(|i| if let Item::Glyphs(g) = i { Some(g) } else { None }).collect();
    assert_eq!(runs.len(), texts.len());
    for (g, t) in runs.iter().zip(&texts) {
        assert_eq!(g.text, t.text);
        assert!((g.origin.y - (baseline - t.rise)).abs() < 0.01, "{}", g.text);
    }
}

#[test]
fn decorations_and_highlights_become_rectangles() {
    let marks = Marks {
        underline: true,
        strike: true,
        background: Some(Color::rgb(255, 245, 157)),
        color: Some(Color::rgb(192, 0, 0)),
        ..Default::default()
    };
    let link = Marks { link: Some("https://example.com".into()), ..Default::default() };
    let l = lay(&doc(vec![para_of(vec![tm("слово", marks), t(" "), tm("лінк", link)])]));
    let page = l.page(0, &fonts()).unwrap();
    let rects: Vec<_> = page
        .items
        .iter()
        .filter_map(|i| if let Item::Rect { rect, color } = i { Some((*rect, *color)) } else { None })
        .collect();
    // Highlight, underline and strike-through of the word, underline of the link.
    assert_eq!(rects.len(), 4);
    assert_eq!(rects[0].1, Color::rgb(255, 245, 157));
    assert_eq!(rects.iter().filter(|r| r.1 == Color::rgb(192, 0, 0)).count(), 2);
    let runs: Vec<_> = page.items.iter().filter_map(|i| if let Item::Glyphs(g) = i { Some(g) } else { None }).collect();
    let link_run = runs.iter().find(|g| g.text == "лінк").unwrap();
    assert_eq!(link_run.color, style::LINK_COLOR);
    assert_eq!(link_run.link.as_deref(), Some("https://example.com"));
    assert!(rects.iter().any(|r| r.1 == style::LINK_COLOR && r.0.y > link_run.origin.y));
    // The highlight is painted before any glyphs.
    assert!(matches!(page.items[0], Item::Rect { .. }));
}

#[test]
fn a_tab_jumps_to_the_next_stop() {
    let p = Block::Paragraph(Paragraph {
        props: ParaProps { indent_first: Some(0.0), align: Some(Align::Left), ..Default::default() },
        inlines: vec![t("a\tb\t\tc")],
        ..Default::default()
    });
    let l = lay(&doc(vec![p]));
    let x = |pos| l.caret_rect(pos).unwrap().x - LEFT;
    // "a" at 1, tab at 2, "b" at 3, tabs at 4 and 5, "c" at 6.
    assert!((x(3) - style::TAB).abs() < 0.01, "{}", x(3));
    assert!((x(5) - 2.0 * style::TAB).abs() < 0.01);
    assert!((x(6) - 3.0 * style::TAB).abs() < 0.01);
}

#[test]
fn forced_breaks_and_code_newlines_end_lines() {
    let brk = Inline::Atom(Atom { kind: AtomKind::Break, width: 0.0, height: 0.0, depth: 0.0, data: None });
    let l = lay(&doc(vec![para_of(vec![t("перший"), brk.clone(), t("другий"), brk])]));
    let ls = lines(&l);
    // A break at the very end leaves an empty third line for the caret.
    assert_eq!(ls.iter().map(|(_, l)| line_text(l)).collect::<Vec<_>>(), vec!["перший", "другий", ""]);
    assert!(ls.iter().all(|(_, l)| l.line.hard));
    assert_eq!((ls[2].1.line.from, ls[2].1.line.to), (14, 14));
    assert!(l.caret_rect(15).unwrap().y > l.caret_rect(13).unwrap().y);

    let code = Block::Paragraph(Paragraph {
        props: ParaProps { code: true, ..Default::default() },
        inlines: vec![t("def f(a):\n    return a  # коментар")],
        ..Default::default()
    });
    let l = lay(&doc(vec![code]));
    let ls = lines(&l);
    assert_eq!(ls.len(), 2);
    // Leading spaces of the second line are kept, in the monospace font: four characters of 0.6 em at 12 pt.
    let second = &ls[1].1.line;
    let indent = l.caret_rect(1 + 10 + 4).unwrap().x - l.caret_rect(1 + 10).unwrap().x;
    assert!((indent - 4.0 * 0.6 * 12.0).abs() < 0.05, "{indent}");
    assert_eq!(second.from, 10);
    assert_eq!(ls[1].1.background, Some(style::CODE_BLOCK_COLOR));
}

#[test]
fn a_word_longer_than_the_line_is_broken_inside() {
    let word = "ш".repeat(400);
    let d = doc(vec![para(&word)]);
    let l = lay(&d);
    let ls = lines(&l);
    assert!(ls.len() >= 6);
    let mut next = 0;
    for (_, pl) in &ls {
        assert!(pl.line.width <= TEXT_W + 0.02);
        assert_eq!(pl.line.from, next);
        assert!(pl.line.to > pl.line.from);
        next = pl.line.to;
    }
    assert_eq!(next, 400);
    assert_eq!(ls.iter().map(|(_, l)| line_text(l)).collect::<String>(), word);
}

#[test]
fn an_object_wider_than_the_page_still_gets_a_line() {
    let wide = Inline::Atom(Atom { kind: AtomKind::Math, width: 2000.0, height: 20.0, depth: 4.0, data: None });
    let l = lay(&doc(vec![para_of(vec![t("до "), wide, t(" після")])]));
    assert_eq!(lines(&l).len(), 3);
    let page = l.page(0, &fonts()).unwrap();
    assert!(
        page.items.iter().any(|i| matches!(i, Item::Object(o) if o.kind == ObjectKind::Math && o.rect.w == 2000.0))
    );
}

#[test]
fn inline_objects_sit_on_the_baseline_and_can_raise_the_line() {
    let math =
        Inline::Atom(Atom { kind: AtomKind::Math, width: 40.0, height: 50.0, depth: 10.0, data: Some("x^2".into()) });
    let l = lay(&doc(vec![para_of(vec![t("формула "), math, t(" тут")])]));
    let pl = lines(&l)[0].1;
    // 40 pt above the baseline is more than the text's ascent: the baseline moves down to make room.
    assert!((pl.line.baseline - 40.0).abs() < 0.01);
    assert!(pl.line.height >= 50.0);
    let page = l.page(0, &fonts()).unwrap();
    let o = page.items.iter().find_map(|i| if let Item::Object(o) = i { Some(o) } else { None }).unwrap();
    assert!((o.rect.y - pl.y).abs() < 0.01 && (o.rect.bottom() - (pl.y + 50.0)).abs() < 0.01);
    assert_eq!((o.pos, o.data.as_deref()), (1 + 8, Some("x^2")));
}

#[test]
fn footnote_marks_are_numbered_through_the_document() {
    let mark = || {
        Inline::Atom(Atom {
            kind: AtomKind::Footnote, width: 0.0, height: 0.0, depth: 0.0, data: Some("текст".into())
        })
    };
    let d = doc(vec![
        para_of(vec![t("один"), mark(), t(" два"), mark()]),
        para("без виносок"),
        para_of(vec![t("три"), mark()]),
    ]);
    let l = lay(&d);
    let marks: Vec<String> = lines(&l)
        .iter()
        .flat_map(|(_, pl)| pl.line.frags.iter())
        .filter_map(|f| if let FragKind::Text(t) = &f.kind { (t.rise > 0.0).then(|| t.text.clone()) } else { None })
        .collect();
    assert_eq!(marks, vec!["1", "2", "3"]);
    // A mark is one position however many digits it shows.
    assert_eq!(d.content_size(), 2 + 4 + 1 + 4 + 1 + 13 + 2 + 4);
}

#[test]
fn list_items_hang_their_markers() {
    let item = |s: &str, more: Vec<Block>| ListItem { blocks: std::iter::once(para(s)).chain(more).collect() };
    let nested = Block::List(List {
        ordered: true,
        style: Some("cyrillic-paren".into()),
        start: 1,
        items: vec![item("вкладений", vec![])],
    });
    let list = Block::List(List {
        ordered: true,
        style: None,
        start: 9,
        items: vec![item("дев'ятий", vec![]), item("десятий", vec![nested, para("продовження")])],
    });
    let bullets =
        Block::List(List {
            ordered: false, style: Some("dash".into()), start: 1, items: vec![item("тире", vec![])]
        });
    let l = lay(&doc(vec![list, bullets]));
    let ls = lines(&l);
    let marker = |i: usize| {
        ls[i].1.line.frags.first().and_then(|f| {
            if let FragKind::Text(t) = &f.kind {
                (f.from == f.to).then(|| (t.text.clone(), ls[i].1.x - LEFT + f.x))
            } else {
                None
            }
        })
    };
    assert_eq!(marker(0).unwrap().0, "9.");
    assert_eq!(marker(1).unwrap().0, "10.");
    assert_eq!(marker(2).unwrap().0, "а)");
    // The second paragraph of an item has no marker of its own.
    assert_eq!(marker(3), None);
    assert_eq!(marker(4).unwrap().0, "–");
    // Text of a first-level item starts 709 twips in, its marker 360 twips before that; a nested level adds 709.
    assert!((ls[0].1.x - LEFT - style::LIST_INDENT).abs() < 0.01 && ls[0].1.line.x.abs() < 0.01);
    assert!((marker(0).unwrap().1 - (style::LIST_INDENT - style::LIST_HANGING)).abs() < 0.01);
    assert!((ls[2].1.x - LEFT - 2.0 * style::LIST_INDENT).abs() < 0.01);
    assert_eq!(
        flow::list_marker(&List { ordered: true, style: Some("upper-roman".into()), start: 1, items: vec![] }, 14),
        "XIV."
    );
    assert_eq!(
        flow::list_marker(&List { ordered: true, style: Some("latin-paren".into()), start: 1, items: vec![] }, 28),
        "ab)"
    );
}

#[test]
fn headings_have_their_own_size_spacing_and_no_indent() {
    let l = lay(&doc(vec![para("Текст."), heading(1, "Розділ"), para("Текст.")]));
    let ls = lines(&l);
    let h = ls[1].1;
    assert!(h.line.x.abs() < 0.01);
    // 12 pt before the heading and 6 pt after it.
    assert!((h.y - (ls[0].1.y + ls[0].1.line.height + 12.0)).abs() < 0.01);
    assert!((ls[2].1.y - (h.y + h.line.height + 6.0)).abs() < 0.01);
    let run = h.line.frags.iter().find_map(|f| if let FragKind::Text(t) = &f.kind { Some(t) } else { None }).unwrap();
    assert_eq!((run.size, run.font), (16.0, fonts().resolve(None, true, false).unwrap().id));
}

fn filler(n: usize) -> Vec<Block> {
    (0..n).map(|i| para(&format!("Рядок {i}"))).collect()
}
/// Body lines per page: 257 mm of text area over 24.15 pt lines.
const LINES_PER_PAGE: usize = 30;

#[test]
fn text_flows_onto_new_pages_and_stays_inside_the_margins() {
    let d = doc(filler(100));
    let l = lay(&d);
    assert_eq!(l.pages.len(), 4);
    for (i, p) in l.pages.iter().enumerate() {
        let n = p.entries.len();
        assert_eq!(n, if i < 3 { LINES_PER_PAGE } else { 10 });
        assert!((p.entries[0].top() - mm(20.0)).abs() < 0.01);
        assert!(p.entries[n - 1].bottom() <= 841.89 - mm(20.0) + 0.02);
    }
    assert_eq!(l.page_of(1), Some(0));
    assert_eq!(l.page_of(d.content_size() - 1), Some(3));
}

#[test]
fn a_page_break_starts_a_new_page_and_keeps_its_own_position() {
    let d = doc(vec![
        para("перша"),
        leaf(LeafKind::PageBreak, 0.0, 0.0),
        para("друга"),
        leaf(LeafKind::PageBreak, 0.0, 0.0),
        leaf(LeafKind::PageBreak, 0.0, 0.0),
        para("четверта"),
    ]);
    let l = lay(&d);
    assert_eq!(l.pages.len(), 4);
    assert_eq!(l.pages[1].entries.len(), 2);
    // Two breaks in a row leave an empty page between them (it holds only the second break).
    assert!(matches!(l.pages[2].entries.as_slice(), [Entry::Box(b)] if b.kind == LeafKind::PageBreak));
    assert_eq!(l.page_of(7), Some(0));
    assert_eq!(l.caret_rect(7).unwrap().page, 0);
    assert!((l.caret_rect(9).unwrap().y - mm(20.0)).abs() < 0.01);
}

#[test]
fn no_single_line_is_left_at_the_bottom_or_the_top_of_a_page() {
    // 28 one-line paragraphs, then a paragraph of several lines: two fit under them, the rest goes over.
    let mut blocks = filler(28);
    blocks.push(para(LOREM));
    let l = lay(&doc(blocks));
    let total = lines(&l).len() - 28;
    assert!(total >= 5);
    assert_eq!(l.pages[0].entries.len(), 30);
    assert_eq!(l.pages[1].entries.len(), total - 2);

    // 29 lines above: only one line would fit, so the whole paragraph starts on the next page.
    let mut blocks = filler(29);
    blocks.push(para(LOREM));
    let l = lay(&doc(blocks));
    assert_eq!(l.pages[0].entries.len(), 29);

    // A three-line paragraph with room for two: taking two would leave one alone on the next page.
    let three = "слово ".repeat(31);
    let mut blocks = filler(28);
    blocks.push(para(three.trim()));
    let l = lay(&doc(blocks));
    assert_eq!(lines(&l).len(), 31);
    assert_eq!(l.pages[0].entries.len(), 28);
    assert_eq!(l.pages[1].entries.len(), 3);
}

#[test]
fn a_heading_is_never_left_alone_at_the_bottom_of_a_page() {
    // The heading fits on the page, but not together with two lines of what follows.
    let mut blocks = filler(28);
    blocks.push(heading(2, "Висновки"));
    blocks.push(para(LOREM));
    let l = lay(&doc(blocks));
    assert_eq!(l.pages[0].entries.len(), 28);
    assert_eq!(line_text(lines(&l)[28].1), "Висновки");
    assert_eq!(lines(&l)[28].0, 1);
    // At the top of a page the space before the heading is dropped.
    assert!((lines(&l)[28].1.y - mm(20.0)).abs() < 0.01);
}

#[test]
fn boxes_are_centred_scaled_to_fit_and_moved_whole() {
    let mut blocks = filler(25);
    blocks.push(leaf(LeafKind::Image, 300.0, 200.0));
    blocks.push(leaf(LeafKind::Image, 1000.0, 500.0));
    blocks.push(leaf(LeafKind::Rule, 0.0, 0.0));
    blocks.push(leaf(LeafKind::Image, 100.0, 4000.0));
    let l = lay(&doc(blocks));
    let boxes: Vec<(usize, &PlacedBox)> = l
        .pages
        .iter()
        .enumerate()
        .flat_map(|(i, p)| {
            p.entries.iter().filter_map(move |e| if let Entry::Box(b) = e { Some((i, b)) } else { None })
        })
        .collect();
    // 200 pt does not fit under 25 lines: the picture goes to page 2, centred.
    assert_eq!(boxes[0].0, 1);
    assert!((boxes[0].1.rect.x - (LEFT + (TEXT_W - 300.0) / 2.0)).abs() < 0.01);
    // Wider than the text area: scaled down to it, proportions kept.
    assert!((boxes[1].1.rect.w - TEXT_W).abs() < 0.01 && (boxes[1].1.rect.h - TEXT_W / 2.0).abs() < 0.01);
    assert!((boxes[2].1.rect.w - TEXT_W).abs() < 0.01 && boxes[2].1.rect.h < 1.0);
    // Taller than a page: scaled to the page height.
    let tall = boxes[3].1.rect;
    assert!((tall.h - (841.89 - 2.0 * mm(20.0))).abs() < 0.02 && tall.w < 100.0);
    let page = l.page(1, &fonts()).unwrap();
    assert_eq!(page.items.iter().filter(|i| matches!(i, Item::Object(o) if o.kind == ObjectKind::Image)).count(), 2);
}

#[test]
fn header_footer_and_page_number_skip_the_title_page() {
    let mut d = doc(filler(70));
    d.running = Running {
        header: "КПІ ім. Ігоря Сікорського".into(),
        footer: "Київ".into(),
        page_number: PageNumber::TopRight,
        title_page: true,
    };
    let l = lay(&d);
    assert_eq!(l.pages.len(), 3);
    assert!(l.pages[0].running.is_empty());
    let texts = |i: usize| l.pages[i].running.iter().map(line_text).collect::<Vec<_>>();
    assert_eq!(texts(1), vec!["КПІ ім. Ігоря Сікорського", "2", "Київ"]);
    assert_eq!(texts(2)[1], "3");
    let number = &l.pages[1].running[1];
    // The number ends at the right margin, above the text area; the footer sits below it.
    assert!((number.line.x + number.line.width - TEXT_W).abs() < 0.01);
    assert!(number.y + number.line.height < mm(20.0));
    assert!(l.pages[1].running[2].y > 841.89 - mm(20.0));
    // Running lines are drawn but hold no document positions.
    assert_eq!(
        l.page(1, &fonts()).unwrap().items.iter().filter(|i| matches!(i, Item::Glyphs(g) if g.text == "2")).count(),
        1
    );
    assert!(l.hit_test(1, 300.0, 40.0).unwrap() > 0);

    d.running = Running {
        header: String::new(),
        footer: String::new(),
        page_number: PageNumber::BottomCenter,
        title_page: false,
    };
    let l = lay(&d);
    let n = &l.pages[0].running[0];
    assert_eq!(line_text(n), "1");
    assert!((n.line.x + n.line.width / 2.0 - TEXT_W / 2.0).abs() < 0.01);
    d.running.page_number = PageNumber::None;
    assert!(lay(&d).pages.iter().all(|p| p.running.is_empty()));
}

#[test]
fn selection_rectangles_follow_the_lines() {
    let d = doc(vec![para(LOREM), para("Другий.")]);
    let l = lay(&d);
    assert!(l.selection_rects(5, 5).is_empty());
    let one = l.selection_rects(5, 10);
    assert_eq!(one.len(), 1);
    assert!((one[0].rect.x - l.caret_rect(5).unwrap().x).abs() < 0.01);
    assert!((one[0].rect.right() - l.caret_rect(10).unwrap().x).abs() < 0.01);
    // Backwards is the same selection.
    assert_eq!(l.selection_rects(10, 5), one);
    // From the start of the second line: nothing is drawn at the end of the first.
    let second = lines(&l)[1].1;
    let from = second.base + second.line.from;
    let part = l.selection_rects(from, from + 5);
    assert_eq!(part.len(), 1);
    assert!((part[0].rect.y - second.y).abs() < 0.01);
    // From the end of a paragraph into the next: the paragraph break itself is selected.
    let para_end = 1 + utf16_len(LOREM);
    let over = l.selection_rects(para_end, para_end + 4);
    assert_eq!(over.len(), 2);
    assert!(over[0].rect.w > 0.0 && over[0].rect.w < 6.0);
    let all = l.selection_rects(0, d.content_size());
    assert_eq!(all.len(), lines(&l).len());
    // Rectangles of consecutive lines touch: no stripes between them.
    for w in all.windows(2) {
        assert!((w[1].rect.y - w[0].rect.bottom()).abs() < 0.01);
    }
}

#[test]
fn up_and_down_keep_the_column_and_home_end_find_the_line() {
    let d = doc(vec![para(LOREM), para("Короткий."), para(LOREM)]);
    let l = lay(&d);
    let start = 60u32;
    let c = l.caret_rect(start).unwrap();
    let down = l.line_move(start, true, c.x).unwrap();
    let cd = l.caret_rect(down).unwrap();
    assert!(cd.y > c.y && (cd.x - c.x).abs() < 6.0, "{c:?} → {cd:?}");
    assert_eq!(l.line_move(down, false, c.x), Some(start));
    // At the edges the caret stays.
    assert_eq!(l.line_move(1, false, 0.0), Some(1));
    let last = d.content_size() - 1;
    assert_eq!(l.line_move(last, true, 0.0), Some(last));
    // Through a short line and back: the goal column brings the caret back to where it was.
    let ls = lines(&l);
    let in_last_long = ls.iter().rev().find(|(_, pl)| pl.line.width > 300.0 && pl.base > 390).unwrap().1;
    let p = in_last_long.base + in_last_long.line.from + 20;
    let goal = l.caret_rect(p).unwrap().x;
    let (mut up, mut steps) = (p, 0);
    while line_text(
        ls.iter()
            .find(|(_, pl)| {
                let (a, b) = Entry::Line((*pl).clone()).span();
                a <= up && up <= b
            })
            .unwrap()
            .1,
    ) != "Короткий."
    {
        up = l.line_move(up, false, goal).unwrap();
        steps += 1;
        assert!(steps < 20);
    }
    let mut back = up;
    for _ in 0..steps {
        back = l.line_move(back, true, goal).unwrap();
    }
    assert_eq!(back, p);

    let (home, end) = l.line_bounds(start).unwrap();
    assert!(home < start && start < end);
    assert_eq!(l.caret_rect(home).unwrap().y, c.y);
    assert_eq!(l.caret_rect(end).unwrap().y, c.y);
    assert!(l.caret_rect(end).unwrap().x > c.x);
}

#[test]
fn an_edit_re_lays_only_its_block_and_repaints_only_changed_pages() {
    let f = fonts();
    let mut d = doc(filler(100));
    let mut lt = Layouter::new();
    let before = lt.layout(&d, &f).unwrap();
    // Type into the last paragraph.
    d.replace_blocks(99, 100, vec![para("Рядок 99 і ще трохи тексту")]).unwrap();
    lt.replaced(99, 100, 1);
    let after = lt.layout(&d, &f).unwrap();
    assert_eq!(after, layout(&d, &f).unwrap());
    let changed: Vec<usize> = (0..4).filter(|i| before.fingerprint(*i) != after.fingerprint(*i)).collect();
    assert_eq!(changed, vec![3]);
    // Untouched lines are the very same allocations: nothing was shaped again.
    let (a, b) = (lines(&before), lines(&after));
    assert!(Arc::ptr_eq(&a[0].1.line, &b[0].1.line) && Arc::ptr_eq(&a[98].1.line, &b[98].1.line));
    assert!(!Arc::ptr_eq(&a[99].1.line, &b[99].1.line));

    // Undo: the page looks as before, so it has its old fingerprint (whatever memory the new lines landed in).
    d.replace_blocks(99, 100, vec![para("Рядок 99")]).unwrap();
    lt.replaced(99, 100, 1);
    let undone = lt.layout(&d, &f).unwrap();
    assert_eq!(undone.fingerprint(3), before.fingerprint(3));
    d.replace_blocks(99, 100, vec![para("Рядок 99 і ще трохи тексту")]).unwrap();
    lt.replaced(99, 100, 1);
    assert_eq!(lt.layout(&d, &f).unwrap().fingerprint(3), after.fingerprint(3));

    // A new first paragraph pushes everything down: every page changes, yet only one block is laid out.
    d.replace_blocks(0, 0, vec![para("Новий перший")]).unwrap();
    lt.replaced(0, 0, 1);
    let pushed = lt.layout(&d, &f).unwrap();
    assert_eq!(pushed, layout(&d, &f).unwrap());
    assert!((0..4).all(|i| after.fingerprint(i) != pushed.fingerprint(i)));
    assert!(Arc::ptr_eq(&lines(&after)[5].1.line, &lines(&pushed)[6].1.line));
}

#[test]
fn a_new_footnote_renumbers_the_marks_after_it() {
    let f = fonts();
    let mark = || Inline::Atom(Atom { kind: AtomKind::Footnote, width: 0.0, height: 0.0, depth: 0.0, data: None });
    let mut d = doc(vec![para("перший"), para_of(vec![t("другий"), mark()])]);
    let mut lt = Layouter::new();
    lt.layout(&d, &f).unwrap();
    d.replace_blocks(0, 1, vec![para_of(vec![t("перший"), mark()])]).unwrap();
    lt.replaced(0, 1, 1);
    let l = lt.layout(&d, &f).unwrap();
    assert_eq!(l, layout(&d, &f).unwrap());
    let marks: Vec<String> = lines(&l)
        .iter()
        .flat_map(|(_, pl)| pl.line.frags.iter())
        .filter_map(|f| if let FragKind::Text(t) = &f.kind { (t.rise > 0.0).then(|| t.text.clone()) } else { None })
        .collect();
    assert_eq!(marks, vec!["1", "2"]);
}

#[test]
fn characters_the_font_lacks_come_from_another_loaded_font_or_stay_one_box() {
    // Liberation Serif has no "↔" glyph? If it does, the test still holds: the run keeps one font.
    let l = lay(&doc(vec![para("до 😀 після")]));
    let texts: Vec<String> = lines(&l)[0]
        .1
        .line
        .frags
        .iter()
        .filter_map(|f| if let FragKind::Text(t) = &f.kind { Some(t.text.clone()) } else { None })
        .collect();
    assert_eq!(texts.concat(), "до 😀 після");
    // Positions: the emoji is two UTF-16 units, the caret has no stop inside it.
    let (a, b, c) = (l.caret_rect(4).unwrap().x, l.caret_rect(5).unwrap().x, l.caret_rect(6).unwrap().x);
    assert_eq!(a, b);
    assert!(c > a);
}

// ---- property tests -------------------------------------------------------------------------------------------

fn arb_words() -> impl Strategy<Value = String> {
    let word = prop_oneof![
        6 => "[а-яіїєґ]{1,9}",
        2 => "[A-Za-z]{1,8}",
        1 => "[0-9]{1,4}[,.;]?",
        1 => Just("😀".to_string()),
        1 => Just("складно-підрядний".to_string()),
        1 => "ш{60,90}",
    ];
    proptest::collection::vec(word, 1..40).prop_map(|w| w.join(" "))
}

fn arb_inlines() -> impl Strategy<Value = Vec<Inline>> {
    let piece = prop_oneof![
        6 => (arb_words(), any::<bool>(), any::<bool>(), prop_oneof![Just(None), Just(Some(10.0f32)), Just(Some(20.0f32))])
            .prop_map(|(s, bold, italic, size)| tm(&s, Marks { bold, italic, size, ..Default::default() })),
        1 => (5.0f32..80.0, 5.0f32..40.0).prop_map(|(w, h)| Inline::Atom(Atom { kind: AtomKind::Math, width: w, height: h, depth: h / 4.0, data: None })),
        1 => Just(Inline::Atom(Atom { kind: AtomKind::Footnote, width: 0.0, height: 0.0, depth: 0.0, data: None })),
        1 => Just(Inline::Atom(Atom { kind: AtomKind::Break, width: 0.0, height: 0.0, depth: 0.0, data: None })),
    ];
    proptest::collection::vec(piece, 0..4)
}

fn arb_block() -> impl Strategy<Value = Block> {
    let p = (
        0u8..=3,
        arb_inlines(),
        prop_oneof![Just(None), Just(Some(Align::Left)), Just(Some(Align::Center)), Just(Some(Align::Right))],
    )
        .prop_map(|(level, inlines, align)| {
            Block::Paragraph(Paragraph { level, inlines, props: ParaProps { align, ..Default::default() } })
        });
    let l = prop_oneof![
        Just(leaf(LeafKind::Rule, 0.0, 0.0)),
        Just(leaf(LeafKind::PageBreak, 0.0, 0.0)),
        Just(leaf(LeafKind::Hidden, 0.0, 0.0)),
        (20.0f32..700.0, 20.0f32..900.0).prop_map(|(w, h)| leaf(LeafKind::Image, w, h)),
    ];
    prop_oneof![8 => p, 2 => l].prop_recursive(2, 12, 3, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 1..3).prop_map(|blocks| Block::Quote(Quote { blocks })),
            (any::<bool>(), proptest::collection::vec(proptest::collection::vec(inner.clone(), 1..3), 1..4)).prop_map(
                |(ordered, items)| {
                    Block::List(List {
                        ordered,
                        style: None,
                        start: 1,
                        items: items.into_iter().map(|blocks| ListItem { blocks }).collect(),
                    })
                }
            ),
            proptest::collection::vec(proptest::collection::vec(proptest::collection::vec(inner, 1..2), 1..3), 1..3)
                .prop_map(|rows| {
                    Block::Table(Table {
                        col_widths: vec![],
                        rows: rows
                            .into_iter()
                            .map(|cells| Row {
                                cells: cells.into_iter().map(|blocks| Cell { blocks, ..Default::default() }).collect(),
                            })
                            .collect(),
                    })
                }),
        ]
    })
}

fn arb_doc() -> impl Strategy<Value = Document> {
    proptest::collection::vec(arb_block(), 1..10).prop_map(doc)
}

/// Positions a caret cannot be told apart at: the second half of a surrogate pair.
fn inside_pairs(d: &Document) -> std::collections::HashSet<u32> {
    let mut out = std::collections::HashSet::new();
    d.walk(|e| {
        if let Event::Text { pos, text } = e {
            let mut at = pos;
            for c in text.text.chars() {
                if c.len_utf16() == 2 {
                    out.insert(at + 1);
                }
                at += c.len_utf16() as u32;
            }
        }
    });
    out
}

thread_local! {
    static FONTS: FontSet = fonts();
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    #[test]
    fn layout_holds_its_invariants_on_any_document(d in arb_doc()) {
        FONTS.with(|f| {
            let l = layout(&d, f).unwrap();
            // Deterministic.
            prop_assert_eq!(&l, &layout(&d, f).unwrap());
            prop_assert!(!l.pages.is_empty());
            let bottom = d.page.height - d.page.margin_bottom;
            let page_h = bottom - d.page.margin_top;
            let pairs = inside_pairs(&d);
            for (pi, page) in l.pages.iter().enumerate() {
                let mut y = d.page.margin_top - 0.01;
                for e in &page.entries {
                    // Entries go down the page in order and stay inside the text area, unless taller than a page.
                    prop_assert!(e.top() >= y - 0.01, "page {} entry at {} above {}", pi, e.top(), y);
                    y = e.bottom();
                    prop_assert!(e.bottom() <= bottom + 0.05 || e.bottom() - e.top() > page_h - 0.05, "page {pi}: entry ends at {} below {bottom}", e.bottom());
                    if let Entry::Line(pl) = e {
                        // Fragments of a line tile its positions without gaps or overlaps.
                        let mut at = pl.line.from;
                        for f in &pl.line.frags {
                            prop_assert_eq!(f.from, at.min(f.from).max(if f.from == f.to { f.from } else { at }));
                            prop_assert_eq!(f.carets.len() as u32, f.to - f.from + 1);
                            at = f.to;
                        }
                        prop_assert_eq!(at, pl.line.to);
                    }
                }
                prop_assert!(l.page(pi, f).is_some());
            }
            // Lines of every paragraph tile its content, and every position has a caret that a click finds again.
            let all = lines(&l);
            let mut failure = None;
            d.walk(|e| {
                if let Event::Textblock { pos, size, .. } = e {
                    let own: Vec<_> = all.iter().filter(|(_, pl)| pl.base == pos + 1).collect();
                    let mut at = 0;
                    for (_, pl) in &own {
                        if pl.line.from != at {
                            failure = Some(format!("paragraph at {pos}: line starts at {} after {at}", pl.line.from));
                        }
                        at = pl.line.to;
                    }
                    if at != size - 2 {
                        failure = Some(format!("paragraph at {pos}: lines end at {at}, content is {}", size - 2));
                    }
                    for p in pos + 1..pos + size {
                        if pairs.contains(&p) {
                            continue;
                        }
                        match l.caret_rect(p) {
                            Some(c) => {
                                let hit = l.hit_test(c.page, c.x, c.y + c.height / 2.0);
                                if hit != Some(p) {
                                    failure = Some(format!("position {p}: caret {c:?} is hit as {hit:?}"));
                                }
                            }
                            None => failure = Some(format!("position {p} has no caret")),
                        }
                    }
                }
            });
            prop_assert!(failure.is_none(), "{}", failure.unwrap_or_default());
            Ok(())
        })?;
    }

    #[test]
    fn incremental_layout_equals_full_layout_after_any_edits(d in arb_doc(), edits in proptest::collection::vec((0usize..12, 0usize..12, proptest::collection::vec(arb_block(), 0..3)), 1..6)) {
        FONTS.with(|f| {
            let mut d = d;
            let mut lt = Layouter::new();
            lt.layout(&d, f).unwrap();
            for (a, b, new) in edits {
                let (from, to) = (a.min(b).min(d.blocks.len()), a.max(b).min(d.blocks.len()));
                let count = new.len();
                d.replace_blocks(from, to, new).unwrap();
                lt.replaced(from, to, count);
                let incremental = lt.layout(&d, f).unwrap();
                prop_assert_eq!(incremental, layout(&d, f).unwrap());
            }
            Ok(())
        })?;
    }
}
