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
    // 14 pt Times: single line 16.1 pt, one and a half 24.15 pt; the extra space goes below the text, the font's
    // own line gap (87 units) stays above it.
    assert!((line.line.height - 24.149).abs() < 0.01, "{}", line.line.height);
    assert!((line.line.baseline - 14.0 * (1825.0 + 87.0) / 2048.0).abs() < 0.01);
    let width = fonts().get(sfdoc_fonts::FontId(0)).unwrap().shaper().unwrap().advance("Привіт", 14.0);
    let start = l.caret_rect(1).unwrap();
    let end = l.caret_rect(7).unwrap();
    assert!((start.x - (LEFT + INDENT)).abs() < 0.01);
    assert!((end.x - start.x - width).abs() < 0.01);
    assert_eq!((start.page, start.y), (0, line.y));
    // The caret spans the single-spaced line, not the whole spaced one.
    assert!((start.height - 14.0 * (1825.0 + 443.0 + 87.0) / 2048.0).abs() < 0.01);
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
    assert!((line.baseline - 24.0 * (1825.0 + 87.0) / 2048.0).abs() < 0.01);
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
fn an_object_wider_than_the_line_is_scaled_to_it() {
    let wide = Inline::Atom(Atom { kind: AtomKind::Math, width: 2000.0, height: 20.0, depth: 4.0, data: None });
    let l = lay(&doc(vec![para_of(vec![t("до "), wide, t(" після")])]));
    let page = l.page(0, &fonts()).unwrap();
    let o = page.items.iter().find_map(|i| if let Item::Object(o) = i { Some(o) } else { None }).unwrap();
    // As wide as a first line can be, proportions kept, inside the text area.
    assert!((o.rect.w - (TEXT_W - INDENT)).abs() < 0.01);
    assert!((o.rect.h / o.rect.w - 20.0 / 2000.0).abs() < 1e-5);
    assert!(o.rect.right() <= LEFT + TEXT_W + 0.01);
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

#[test]
fn space_after_and_space_before_do_not_add_up() {
    // Two headings in a row: 6 pt after the first, 12 pt before the second - the gap is the larger one.
    let l = lay(&doc(vec![para("Текст."), heading(1, "Розділ"), heading(2, "Підрозділ")]));
    let ls = lines(&l);
    let (h1, h2) = (ls[1].1, ls[2].1);
    assert!((h2.y - (h1.y + h1.line.height + 12.0)).abs() < 0.01, "gap {}", h2.y - h1.y - h1.line.height);
}

fn aligned(s: &str, align: Align) -> Block {
    Block::Paragraph(Paragraph {
        inlines: vec![t(s)],
        props: ParaProps { align: Some(align), ..Default::default() },
        ..Default::default()
    })
}

#[test]
fn a_justified_line_narrows_its_spaces_to_take_one_more_word() {
    // The same text set flush left and justified, at many lengths of the first word so that some line is bound to
    // miss its last word by a little.
    let mut squeezed = 0;
    for n in 1..40 {
        let text = format!("{} {LOREM}", "а".repeat(n));
        let left = lay(&doc(vec![aligned(&text, Align::Left)]));
        let just = lay(&doc(vec![aligned(&text, Align::Justify)]));
        let (ll, jl) = (lines(&left), lines(&just));
        assert!(jl.len() <= ll.len());
        for (i, (_, pl)) in jl.iter().enumerate() {
            let room = TEXT_W - if i == 0 { INDENT } else { 0.0 };
            let last = pl.line.frags.last().unwrap();
            let ink_right = last.x + if let FragKind::Text(t) = &last.kind { t.ink_width } else { last.advance };
            // Squeezed or stretched, the text ends at the right edge and never beyond it - the last line too.
            assert!(ink_right <= TEXT_W + 0.02, "n {n} line {i} ends at {ink_right}");
            if pl.line.width > room + 0.02 {
                squeezed += 1;
                assert!((ink_right - TEXT_W).abs() < 0.02);
                // No more than a quarter of the spaces' width is given up.
                let spaces = line_text(pl).trim_end().matches(' ').count() as f32 * 3.5;
                assert!(pl.line.width - room <= spaces * 0.25 + 0.01, "n {n} line {i}: {} over", pl.line.width - room);
                // The carets follow the narrowed spaces: the caret after the last letter is where the ink ends.
                let end = pl.base + pl.line.from + line_text(pl).trim_end().encode_utf16().count() as u32;
                let c = just.caret_rect(end).unwrap();
                assert!((c.x - (LEFT + ink_right)).abs() < 0.05, "caret {} ink {}", c.x, LEFT + ink_right);
            }
        }
        assert_eq!(jl.iter().map(|(_, l)| line_text(l)).collect::<String>(), text);
    }
    assert!(squeezed >= 3, "only {squeezed} squeezed lines in 39 texts");
}

#[test]
fn the_last_line_of_a_page_needs_room_for_its_text_only() {
    // Double spacing: 32.2 pt lines. 22 whole lines fit the text area; the 23rd has room for its text (16.1 pt)
    // but not for the leading under it, and still stays on the page.
    let blocks: Vec<Block> = (0..50)
        .map(|i| {
            Block::Paragraph(Paragraph {
                inlines: vec![t(&format!("Рядок {i}"))],
                props: ParaProps { line_height: Some(2.0), ..Default::default() },
                ..Default::default()
            })
        })
        .collect();
    let l = lay(&doc(blocks));
    let first: Vec<_> = lines(&l).into_iter().filter(|(p, _)| *p == 0).collect();
    assert_eq!(first.len(), 23);
    let last = first[22].1;
    let bottom = 841.89 - mm(20.0);
    assert!(last.y + last.line.baseline + last.line.descent <= bottom + 0.01);
    assert!(last.y + last.line.height > bottom);
}

/// Where the content of an entry ends: a line may hang the leading under its text past the text area.
fn text_bottom(e: &Entry) -> f32 {
    match e {
        Entry::Line(l) => l.y + (l.line.baseline + l.line.descent).min(l.line.height),
        _ => e.bottom(),
    }
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

fn cell_of(blocks: Vec<Block>) -> Cell {
    Cell { blocks, ..Default::default() }
}
fn cell(s: &str) -> Cell {
    cell_of(vec![para(s)])
}
fn head(s: &str) -> Cell {
    Cell { header: true, ..cell(s) }
}
fn table(col_widths: Vec<f32>, rows: Vec<Vec<Cell>>) -> Block {
    Block::Table(Table { col_widths, rows: rows.into_iter().map(|cells| Row { cells }).collect() })
}
fn boxes(l: &Layout) -> Vec<(usize, &PlacedBox)> {
    l.pages
        .iter()
        .enumerate()
        .flat_map(|(i, p)| {
            p.entries.iter().filter_map(move |e| if let Entry::Box(b) = e { Some((i, b)) } else { None })
        })
        .collect()
}

#[test]
fn table_cells_stand_side_by_side_in_a_grid() {
    let d = doc(vec![
        para("До таблиці."),
        table(
            vec![100.0, 200.0],
            vec![
                vec![head("Алгоритм"), head("Час")],
                vec![
                    cell("Бульбашка"),
                    cell("довгий текст, якого вистачить на кілька рядків у вузькій клітинці таблиці"),
                ],
            ],
        ),
        para("Після."),
    ]);
    let l = lay(&d);
    let page = &l.pages[0];
    assert_eq!(page.cells.len(), 4);
    let [a, b, c, e] = [page.cells[0], page.cells[1], page.cells[2], page.cells[3]];
    // The document's own column widths, from the left edge of the text area.
    assert!((a.x - LEFT).abs() < 0.01 && (a.w - 100.0).abs() < 0.01);
    assert!((b.x - (LEFT + 100.0)).abs() < 0.01 && (b.w - 200.0).abs() < 0.01);
    // Cells of a row share its top and height; the second row starts where the first ends.
    assert_eq!((a.y, a.h), (b.y, b.h));
    assert_eq!((c.y, c.h), (e.y, e.h));
    assert!((c.y - a.bottom()).abs() < 0.01);
    // The long cell wraps and sets the row's height; its neighbour is as tall with one line.
    let in_cell = |r: u16| lines(&l).into_iter().filter(|(_, pl)| pl.region == r).collect::<Vec<_>>();
    assert_eq!(in_cell(3).len(), 1);
    assert!(in_cell(4).len() >= 3);
    assert!((c.h - (in_cell(4).iter().map(|(_, pl)| pl.line.height).sum::<f32>() + flow::CELL_EXTRA)).abs() < 0.01);
    // Text starts after the cell padding; header cells are bold, body cells are not, single-spaced and unindented.
    let first = in_cell(1)[0].1;
    assert!((first.x - (LEFT + flow::CELL_PAD_X)).abs() < 0.01 && first.line.x.abs() < 0.01);
    assert!((first.y - (a.y + flow::CELL_TOP)).abs() < 0.01);
    let font = |pl: &PlacedLine| {
        pl.line.frags.iter().find_map(|f| if let FragKind::Text(t) = &f.kind { Some(t.font) } else { None }).unwrap()
    };
    assert_eq!(font(first), fonts().resolve(None, true, false).unwrap().id);
    assert_eq!(font(in_cell(3)[0].1), fonts().resolve(None, false, false).unwrap().id);
    assert!((in_cell(3)[0].1.line.height - 14.0 * 2355.0 / 2048.0).abs() < 0.01);
    // A frame of four hairlines per cell, drawn under the text.
    assert_eq!(page.decor.len(), 16);
    let display = l.page(0, &fonts()).unwrap();
    assert!(display.items.iter().take(16).all(|i| matches!(i, Item::Rect { color, .. } if *color == Color::BLACK)));
    // The paragraphs around keep a gap from the table.
    let body = in_cell(0);
    assert!(a.y - (body[0].1.y + body[0].1.line.height) >= 5.9);
    assert!(body[1].1.y - e.bottom() >= 5.9);
}

#[test]
fn columns_without_widths_follow_their_content_and_fill_the_text_area() {
    let l = lay(&doc(vec![table(
        vec![],
        vec![vec![cell("№"), cell("Назва алгоритму сортування")], vec![cell("1"), cell("Швидке")]],
    )]));
    let c = &l.pages[0].cells;
    assert!((c[0].w + c[1].w - TEXT_W).abs() < 0.05);
    assert!(c[1].w > 3.0 * c[0].w, "{} vs {}", c[0].w, c[1].w);
    // Nothing wraps: every cell is one line.
    assert_eq!(lines(&l).len(), 4);

    // Too much for the page: columns shrink in proportion, the table still ends at the right margin.
    let wide = "дуже довгий вміст клітинки, якому не вистачить місця в один рядок на сторінці ".repeat(2);
    let l = lay(&doc(vec![table(vec![], vec![vec![cell(&wide), cell("коротко"), cell(&wide)]])]));
    let c = &l.pages[0].cells;
    assert!((c[2].right() - (LEFT + TEXT_W)).abs() < 0.05);
    assert!(c[1].w < c[0].w);
    // The short column keeps its word whole: columns give up only what they have beyond their longest word.
    assert_eq!(lines(&l).iter().filter(|(_, pl)| pl.region == 2).count(), 1);
    let l = lay(&doc(vec![table(vec![], vec![vec![cell("Бульбашкою"), cell(&wide), cell("1250")]])]));
    let texts: Vec<String> = lines(&l).iter().map(|(_, pl)| line_text(pl)).collect();
    assert!(texts.contains(&"Бульбашкою".to_string()) && texts.contains(&"1250".to_string()), "{texts:?}");
    // Stated widths wider than the page are scaled down too.
    let l = lay(&doc(vec![table(vec![600.0, 600.0], vec![vec![cell("a"), cell("b")]])]));
    assert!((l.pages[0].cells[1].right() - (LEFT + TEXT_W)).abs() < 0.05);
}

#[test]
fn column_widths_are_reported_in_document_order() {
    let inner = table(vec![], vec![vec![cell("a"), cell("b"), cell("c")]]);
    let d = doc(vec![
        table(vec![100.0, 200.0], vec![vec![cell("1"), cell("2")]]),
        para("Між таблицями."),
        table(vec![], vec![vec![cell("№"), cell_of(vec![para("Вкладена:"), inner])]]),
    ]);
    let mut layouter = Layouter::new();
    let l = layouter.layout(&d, &fonts()).unwrap();
    let cols = layouter.table_columns();
    assert_eq!(cols.iter().map(Vec::len).collect::<Vec<_>>(), vec![2, 2, 3]);
    assert!((cols[0][0] - 100.0).abs() < 0.01 && (cols[0][1] - 200.0).abs() < 0.01, "{:?}", cols[0]);
    // The widths are those of the drawn cells: the second table fills the text area.
    assert!((cols[1].iter().sum::<f32>() - TEXT_W).abs() < 0.05);
    assert!((cols[1][0] - l.pages[0].cells[2].w).abs() < 0.01, "{:?} {:?}", cols[1], l.pages[0].cells);
    // The nested table fits inside its cell.
    assert!(cols[2].iter().sum::<f32>() < cols[1][1]);
}

#[test]
fn merged_cells_span_columns_and_rows() {
    let tall = Cell { rowspan: 2, ..cell("на два рядки") };
    let wide = Cell { colspan: 2, ..cell("на два стовпці") };
    let l = lay(&doc(vec![table(
        vec![100.0, 100.0, 100.0],
        vec![vec![tall, cell("b"), cell("c")], vec![cell("d"), cell("e")], vec![wide, cell("f")]],
    )]));
    let c = &l.pages[0].cells;
    assert_eq!(c.len(), 7);
    // The row-spanning cell covers both rows; the second row's cells start in the second column.
    assert!((c[0].h - (c[1].h + c[3].h)).abs() < 0.01);
    assert!((c[3].x - (LEFT + 100.0)).abs() < 0.01 && (c[4].x - (LEFT + 200.0)).abs() < 0.01);
    assert!((c[5].w - 200.0).abs() < 0.01 && (c[6].x - (LEFT + 200.0)).abs() < 0.01);
    // A spanning cell taller than its rows stretches the last of them.
    let big = Cell { rowspan: 2, ..cell_of((0..6).map(|i| para(&format!("рядок {i}"))).collect()) };
    let l = lay(&doc(vec![table(vec![200.0, 100.0], vec![vec![big, cell("x")], vec![cell("y")]])]));
    let c = &l.pages[0].cells;
    assert!(c[2].h > 3.0 * c[1].h);
    assert!((c[0].h - (c[1].h + c[2].h)).abs() < 0.01);
}

#[test]
fn clicks_and_vertical_movement_respect_cells() {
    let long = "текст на кілька рядків у вузькому стовпці таблиці";
    let d = doc(vec![
        para("Перед."),
        table(vec![150.0, 150.0], vec![vec![cell("ліва"), cell(long)], vec![cell("нижня ліва"), cell("нижня права")]]),
        para("Після."),
    ]);
    let l = lay(&d);
    let c = l.pages[0].cells.clone();
    let pos_of = |needle: &str| {
        let mut found = None;
        d.walk(|e| {
            if let Event::Text { pos, text } = e
                && text.text.starts_with(needle)
            {
                found = Some(pos);
            }
        });
        found.unwrap()
    };
    let (left, right, below, below_right) =
        (pos_of("ліва"), pos_of("текст"), pos_of("нижня ліва"), pos_of("нижня права"));
    // Under the short left cell's only line, level with the right cell's third line: still the left cell.
    let hit = l.hit_test(0, c[0].x + 60.0, c[0].bottom() - 3.0).unwrap();
    assert!(hit >= left && hit <= left + 4, "{hit}");
    // Every caret in a cell is found again by a click on it.
    for p in right..right + utf16_len(long) {
        let caret = l.caret_rect(p).unwrap();
        assert_eq!(l.hit_test(0, caret.x, caret.y + caret.height / 2.0), Some(p));
    }
    // Down from the left cell: the cell below it, not the cell to the right (which is next in the document).
    let x = l.caret_rect(left + 2).unwrap().x;
    let down = l.line_move(left + 2, true, x).unwrap();
    assert!(down >= below && down < below + 10, "{down}");
    assert_eq!(l.line_move(down, false, x), Some(left + 2));
    // Down the right column line by line, then into the cell below, then out of the table.
    let xr = l.caret_rect(right + 3).unwrap().x;
    let mut p = right + 3;
    let mut seen = vec![];
    for _ in 0..8 {
        p = l.line_move(p, true, xr).unwrap();
        seen.push(p);
    }
    assert!(seen.iter().any(|p| *p >= below_right && *p < below_right + 12));
    assert!(seen.iter().all(|p| !(*p >= below && *p < below + 10)), "strayed into the left column: {seen:?}");
    assert_eq!(*seen.last().unwrap(), l.line_move(*seen.last().unwrap(), true, xr).unwrap());
    assert!(*seen.last().unwrap() > below_right + 12);
    // Up from the paragraph after the table at the left column's x: into the bottom-left cell.
    let after = pos_of("Після.");
    let up = l.line_move(after, false, c[0].x + 20.0).unwrap();
    assert!(up >= below && up < below + 10, "{up}");
}

#[test]
fn a_long_table_moves_rows_whole_and_repeats_its_header() {
    let mut rows = vec![vec![head("№"), head("Значення")]];
    rows.extend((0..80).map(|i| vec![cell(&format!("{i}")), cell(&format!("значення {i}"))]));
    let d = doc(vec![table(vec![60.0, 200.0], rows)]);
    let l = lay(&d);
    assert!(l.pages.len() >= 3);
    for (i, page) in l.pages.iter().enumerate() {
        // No cell is cut by the page's bottom edge.
        assert!(page.cells.iter().all(|c| c.bottom() <= 841.89 - mm(20.0) + 0.05));
        let ghosts: Vec<String> = page.running.iter().map(line_text).collect();
        if i == 0 {
            assert!(ghosts.is_empty());
            assert_eq!(line_text(lines(&l)[0].1), "№");
        } else {
            // The header again, drawn at the top, above the first real row.
            assert_eq!(ghosts[..2], ["№", "Значення"]);
            assert!((page.running[0].y - (mm(20.0) + flow::CELL_TOP)).abs() < 0.01);
            assert!(page.entries[0].top() > page.running[0].y + 10.0);
        }
    }
    // The repeated header holds no positions: the caret of the header text is on page 1 only.
    assert_eq!(l.page_of(4), Some(0));
    let total: usize = l.pages.iter().map(|p| p.entries.len()).sum();
    assert_eq!(total, 2 + 160);
    // The repeat is painted: page 2's display list has the header text although no entry holds it.
    let display = l.page(1, &fonts()).unwrap();
    assert!(display.items.iter().any(|i| matches!(i, Item::Glyphs(g) if g.text == "Значення")));
}

#[test]
fn a_row_taller_than_a_page_continues_on_the_next() {
    let many: Vec<Block> = (0..100).map(|i| para(&format!("рядок {i}"))).collect();
    let d = doc(vec![
        para("Перед."),
        table(vec![200.0, 200.0], vec![vec![cell_of(many), cell("коротка")], vec![cell("останній"), cell("рядок")]]),
    ]);
    let l = lay(&d);
    assert_eq!(l.pages.len(), 3);
    let bottom = 841.89 - mm(20.0);
    // The first two pages end with the row's frame at the bottom margin; the text continues in order.
    assert!((l.pages[0].cells[0].bottom() - bottom).abs() < 0.05);
    assert!((l.pages[1].cells[0].y - mm(20.0)).abs() < 0.05 && (l.pages[1].cells[0].bottom() - bottom).abs() < 0.05);
    let texts: Vec<String> =
        lines(&l).iter().map(|(_, pl)| line_text(pl)).filter(|t| t.starts_with("рядок ")).collect();
    assert_eq!(texts, (0..100).map(|i| format!("рядок {i}")).collect::<Vec<_>>());
    assert!(lines(&l).iter().all(|(_, pl)| pl.y + pl.line.height <= bottom + 0.05));
    // The short neighbour is drawn once, on the first page; the last row follows the tall one.
    assert_eq!(
        lines(&l).iter().filter(|(_, pl)| line_text(pl) == "коротка").map(|(p, _)| *p).collect::<Vec<_>>(),
        vec![0]
    );
    assert_eq!(lines(&l).iter().find(|(_, pl)| line_text(pl) == "останній").unwrap().0, 2);
}

#[test]
fn pictures_lists_and_tables_live_inside_cells() {
    let inner = table(vec![], vec![vec![cell("в1"), cell("в2")]]);
    let item = |s: &str| ListItem { blocks: vec![para(s)] };
    let list =
        Block::List(List { ordered: true, style: None, start: 1, items: vec![item("один"), item("два")] });
    let d = doc(vec![table(
        vec![240.0, 240.0],
        vec![vec![
            cell_of(vec![para("зверху"), leaf(LeafKind::Image, 600.0, 300.0), list]),
            cell_of(vec![inner, para("під вкладеною")]),
        ]],
    )]);
    let l = lay(&d);
    let c = &l.pages[0].cells;
    // Two outer cells and the nested table's two.
    assert_eq!(c.len(), 4);
    assert!(c[2].x >= c[1].x && c[3].right() <= c[1].right() + 0.01 && c[2].y >= c[1].y);
    // The picture is scaled to the cell's inner width and stays inside the cell.
    let (_, img) = boxes(&l)[0];
    assert!((img.rect.w - (240.0 - 2.0 * flow::CELL_PAD_X)).abs() < 0.01);
    assert!(img.rect.x >= c[0].x && img.rect.right() <= c[0].right() && img.region == 1);
    // A click on the nested cell answers from the nested cell.
    let nested_line = lines(&l).into_iter().find(|(_, pl)| line_text(pl) == "в2").unwrap().1.clone();
    assert_eq!(nested_line.region, 4);
    assert_eq!(l.hit_test(0, c[3].x + 8.0, c[3].y + 8.0), Some(nested_line.base));
    // List markers hang inside the cell.
    let one = lines(&l).into_iter().find(|(_, pl)| line_text(pl).starts_with("1.")).unwrap().1.clone();
    assert!(one.x > c[0].x + style::LIST_INDENT);
}

fn note(text: &str) -> Inline {
    Inline::Atom(Atom { kind: AtomKind::Footnote, width: 0.0, height: 0.0, depth: 0.0, data: Some(text.into()) })
}
/// Texts of the drawn-only lines of a page, the page number (added last) left out.
fn running_texts(l: &Layout, page: usize) -> Vec<String> {
    let all: Vec<String> = l.pages[page].running.iter().map(line_text).collect();
    let number = (page + 1).to_string();
    match all.split_last() {
        Some((last, rest)) if page > 0 && *last == number => rest.to_vec(),
        _ => all,
    }
}
const FLOOR: f32 = 841.8898 - 56.692_917;

#[test]
fn footnotes_sit_at_the_bottom_of_the_page_that_has_their_mark() {
    let d = doc(vec![
        para_of(vec![
            t("Перше твердження"),
            note("Джерело перше."),
            t(" і друге"),
            note("Джерело друге, з довшим текстом."),
        ]),
        para("Далі."),
    ]);
    let l = lay(&d);
    let page = &l.pages[0];
    assert_eq!(running_texts(&l, 0), vec!["1 Джерело перше.", "2 Джерело друге, з довшим текстом."]);
    // 10 pt, single spacing; the last note ends exactly at the bottom margin; a short rule above the first.
    let notes = &page.running;
    assert!((notes[1].y + notes[1].line.height - FLOOR).abs() < 0.01);
    assert!((notes[0].line.height - 10.0 * 2355.0 / 2048.0).abs() < 0.01);
    assert!((notes[0].x - LEFT).abs() < 0.01);
    let rule = page.decor.last().unwrap().0;
    assert!((rule.w - TEXT_W / 3.0).abs() < 0.01 && rule.y < notes[0].y && rule.y > notes[0].y - flow::NOTE_SEPARATOR);
    // The mark in the note is a superscript like the one in the text.
    let first =
        notes[0].line.frags.iter().find_map(|f| if let FragKind::Text(t) = &f.kind { Some(t) } else { None }).unwrap();
    assert!(first.text == "1" && first.rise > 0.0);
    // Notes hold no positions: a click on them answers from the text above.
    let hit = l.hit_test(0, LEFT + 30.0, FLOOR - 5.0).unwrap();
    assert!(hit >= d.content_size() - 6, "{hit}");
    let display = l.page(0, &fonts()).unwrap();
    assert!(display.items.iter().any(|i| matches!(i, Item::Glyphs(g) if g.text.starts_with("Джерело"))));
}

#[test]
fn a_line_moves_to_the_next_page_together_with_its_footnote() {
    // 29 lines, then a one-line paragraph with a footnote: the line alone would fit as the 30th, but not with its
    // note under it.
    let mut blocks = filler(29);
    blocks.push(para_of(vec![t("З виноскою"), note("Текст виноски.")]));
    blocks.push(para("Після."));
    let l = lay(&doc(blocks));
    assert_eq!(l.pages[0].entries.len(), 29);
    assert!(running_texts(&l, 0).is_empty());
    assert_eq!(running_texts(&l, 1), vec!["1 Текст виноски."]);
    assert_eq!(line_text(lines(&l)[29].1), "З виноскою1");

    // With room for both, text after the marked line stops above the notes.
    let mut blocks = filler(20);
    blocks.push(para_of(vec![t("З виноскою"), note(&"Довга виноска на кілька рядків. ".repeat(12))]));
    blocks.extend(filler(20));
    let l = lay(&doc(blocks));
    let notes_top = l.pages[0].running.iter().map(|n| n.y).fold(f32::MAX, f32::min);
    assert!(l.pages[0].running.len() >= 3);
    assert!(l.pages[0].entries.iter().all(|e| text_bottom(e) <= notes_top - flow::NOTE_SEPARATOR + 0.05));
    assert!(l.pages[0].entries.len() < 30 && l.pages.len() == 2);
}

#[test]
fn footnotes_from_table_cells_and_later_pages_keep_their_numbers() {
    let mut blocks = vec![
        para_of(vec![t("Перша"), note("один")]),
        table(
            vec![200.0, 200.0],
            vec![vec![cell_of(vec![para_of(vec![t("у клітинці"), note("два")])]), cell("поруч")]],
        ),
    ];
    blocks.extend(filler(40));
    blocks.push(para_of(vec![t("На другій сторінці"), note("три")]));
    let l = lay(&doc(blocks));
    assert_eq!(running_texts(&l, 0), vec!["1 один", "2 два"]);
    assert_eq!(running_texts(&l, 1), vec!["3 три"]);
    // Notes span the text area even when the mark is in a narrow cell.
    assert!((l.pages[0].running[1].box_w - TEXT_W).abs() < 0.01);
}

fn toc(title: &str) -> Block {
    Block::Leaf(Leaf {
        kind: LeafKind::Toc,
        width: 0.0,
        height: 0.0,
        data: (!title.is_empty()).then(|| title.to_owned()),
    })
}

#[test]
fn the_table_of_contents_lists_headings_with_dots_and_real_page_numbers() {
    let mut blocks = vec![toc("Зміст"), leaf(LeafKind::PageBreak, 0.0, 0.0), heading(1, "Вступ")];
    blocks.extend(filler(40));
    blocks.push(heading(2, "Метод дослідження"));
    blocks.extend(filler(40));
    blocks.push(heading(1, "Висновки"));
    let d = doc(blocks);
    let l = lay(&d);
    let rows = running_texts(&l, 0);
    assert_eq!(rows[0], "Зміст");
    // Title, then per heading: text, dots, number (dots and numbers are added last, after all the text).
    assert_eq!(rows[1..4], ["Вступ", "Метод дослідження", "Висновки"]);
    let numbers: Vec<&String> = rows[4..].iter().filter(|r| !r.starts_with('.')).collect();
    assert_eq!(numbers, vec!["2", "3", "4"]);
    assert!(rows[4..].iter().filter(|r| r.starts_with("....")).count() == 3);
    let r = &l.pages[0].running;
    let find = |s: &str| r.iter().find(|pl| line_text(pl) == s).unwrap();
    // The title is centred and bold; a level-2 row is indented; numbers end at the right margin on the row's line.
    let title = find("Зміст");
    assert!((title.x + title.line.x + title.line.width / 2.0 - (LEFT + TEXT_W / 2.0)).abs() < 0.01);
    assert!((find("Метод дослідження").line.x - 35.433).abs() < 0.01 && find("Вступ").line.x.abs() < 0.01);
    let (two, intro) = (find("2"), find("Вступ"));
    assert!((two.x + two.line.x + two.line.width - (LEFT + TEXT_W)).abs() < 0.01);
    assert_eq!(two.y, intro.y);
    // Dots run between the text and the number without touching either.
    let dots = r.iter().find(|pl| line_text(pl).starts_with("....") && pl.y == intro.y).unwrap();
    let (d0, d1) = (dots.x + dots.line.x, dots.x + dots.line.x + dots.line.width);
    assert!(d0 > intro.x + intro.line.x + intro.line.width + 2.0 && d1 < two.x + two.line.x - 2.0, "{d0}..{d1}");
    assert!(d1 > two.x + two.line.x - 8.0);
    // The whole block is one position: a caret before it and after it, a click inside answers with it.
    let box_ = boxes(&l).into_iter().find(|(_, b)| b.kind == LeafKind::Toc).unwrap().1;
    assert_eq!(box_.pos, 0);
    assert!(box_.rect.h > 4.0 * 24.0);
    assert_eq!(l.hit_test(0, LEFT + 10.0, box_.rect.y + 30.0), Some(0));

    // A renamed heading and a heading that moved to another page both show up, through the cache as well.
    let f = fonts();
    let mut lt = Layouter::new();
    let mut d2 = d.clone();
    lt.layout(&d2, &f).unwrap();
    d2.replace_blocks(2, 3, vec![heading(1, "Вступ до теми")]).unwrap();
    lt.replaced(2, 3, 1);
    let renamed = lt.layout(&d2, &f).unwrap();
    assert_eq!(renamed, layout(&d2, &f).unwrap());
    assert!(renamed.pages[0].running.iter().any(|pl| line_text(pl) == "Вступ до теми"));
    d2.replace_blocks(3, 3, filler(40)).unwrap();
    lt.replaced(3, 3, 40);
    let pushed = lt.layout(&d2, &f).unwrap();
    assert_eq!(pushed, layout(&d2, &f).unwrap());
    let nums: Vec<String> =
        pushed.pages[0].running.iter().map(line_text).filter(|s| s.chars().all(|c| c.is_ascii_digit())).collect();
    assert_eq!(nums, vec!["2", "4", "6"]);
}

#[test]
fn a_table_of_contents_without_headings_or_longer_than_a_page() {
    let l = lay(&doc(vec![toc(""), para("Текст.")]));
    assert!(l.pages[0].running.is_empty());
    assert_eq!(l.caret_rect(0).unwrap().page, 0);
    assert_eq!(l.hit_test(0, LEFT, 0.0), Some(0));
    // 70 headings: the contents take more than a page and continue on the next one.
    let mut blocks = vec![toc("Зміст")];
    blocks.extend((0..70).map(|i| heading(3, &format!("Пункт {i}"))));
    let l = lay(&doc(blocks));
    let on = |p: usize| running_texts(&l, p).iter().filter(|r| r.starts_with("Пункт")).count();
    assert_eq!(on(0), 29);
    assert_eq!(on(0) + on(1) + on(2), 70);
    assert!(l.pages[0].running.iter().all(|pl| pl.y + pl.line.height <= FLOOR + 0.05));
    // A long heading wraps before the number column.
    let long = "Дуже довга назва розділу, яка не вміщується в один рядок змісту і тому переноситься на наступний";
    let l = lay(&doc(vec![toc(""), heading(1, long)]));
    let r = &l.pages[0].running;
    assert!(
        r.iter().filter(|pl| long.starts_with(line_text(pl).trim()) || long.ends_with(line_text(pl).trim())).count()
            >= 2
    );
    let number = r.iter().find(|pl| line_text(pl) == "1").unwrap();
    let last_text = r.iter().rfind(|pl| long.ends_with(line_text(pl).trim()) && !line_text(pl).is_empty()).unwrap();
    assert_eq!(number.y, last_text.y);
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
        1 => "[а-я ]{0,60}".prop_map(|s| Inline::Atom(Atom { kind: AtomKind::Footnote, width: 0.0, height: 0.0, depth: 0.0, data: Some(s) })),
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
        Just(Block::Leaf(Leaf { kind: LeafKind::Toc, width: 0.0, height: 0.0, data: Some("Зміст".into()) })),
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
                // Entries of one region (the page body, or one table cell) go down the page in order, and all stay
                // inside the text area unless taller than a page.
                let mut last: std::collections::HashMap<u16, f32> = std::collections::HashMap::new();
                for e in &page.entries {
                    let y = last.get(&e.region()).copied().unwrap_or(d.page.margin_top - 0.01);
                    prop_assert!(e.top() >= y - 0.01, "page {} entry at {} above {}", pi, e.top(), y);
                    prop_assert!(e.top() >= d.page.margin_top - 0.01, "page {} entry at {} above the text area", pi, e.top());
                    last.insert(e.region(), e.bottom());
                    if e.region() != 0 {
                        let cell = page.cells[usize::from(e.region()) - 1];
                        prop_assert!(e.top() >= cell.y - 0.01 && e.bottom() <= cell.bottom() + 0.05, "page {pi}: entry {}..{} outside its cell {cell:?}", e.top(), e.bottom());
                    }
                    // Known limit: a table inside a cell is never split, so one taller than a page runs past the bottom.
                    let nested = e.region() != 0 && {
                        let own = page.cells[usize::from(e.region()) - 1];
                        page.cells.iter().take(usize::from(e.region()) - 1).any(|o| o.x <= own.x && o.right() >= own.right() - 0.01 && o.y <= own.y && o.h > own.h)
                    };
                    prop_assert!(nested || text_bottom(e) <= bottom + 0.05 || e.bottom() - e.top() > page_h - 0.05, "page {pi}: entry {:?} region {} top {} ends at {} below {bottom}; cells {:?}", e.span(), e.region(), e.top(), e.bottom(), page.cells);
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
                                    let around: Vec<String> = all.iter().filter(|(_, pl)| { let (a, b) = Entry::Line((*pl).clone()).span(); a <= p + 1 && p <= b + 1 }).map(|(pg, pl)| format!("[page {pg} region {} base {} {}..{} hard {} y {} x {} w {} frags {:?}]", pl.region, pl.base, pl.line.from, pl.line.to, pl.line.hard, pl.y, pl.x + pl.line.x, pl.line.width, pl.line.frags.iter().map(|f| (f.from, f.to, f.x, f.advance, match &f.kind { FragKind::Text(t) => t.text.clone(), FragKind::Atom { kind, .. } => format!("{kind:?}"), FragKind::Space => "space".into() })).collect::<Vec<_>>())).collect();
                                    failure = Some(format!("position {p}: caret {c:?} is hit as {hit:?}; lines {around:?}; cells {:?}", l.pages[c.page].cells));
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
