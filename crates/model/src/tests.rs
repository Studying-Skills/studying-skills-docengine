use proptest::prelude::*;

use super::*;

fn text(s: &str) -> Inline {
    Inline::Text(Text { text: s.to_owned(), marks: Marks::default() })
}
fn para(s: &str) -> Block {
    Block::Paragraph(Paragraph { inlines: if s.is_empty() { vec![] } else { vec![text(s)] }, ..Default::default() })
}
fn leaf(kind: LeafKind) -> Block {
    Block::Leaf(Leaf { kind, width: 0.0, height: 0.0, data: None })
}
fn doc(blocks: Vec<Block>) -> Document {
    Document { blocks, ..Default::default() }
}

#[test]
fn paragraph_positions_follow_the_flat_token_scheme() {
    let d = doc(vec![para("ab"), para("")]);
    assert_eq!(d.content_size(), 6);
    assert_eq!(d.text_between(0, 6, "\n"), "ab\n");
    assert_eq!(d.text_between(1, 2, "\n"), "a");
    assert_eq!(d.text_between(2, 3, "\n"), "b");
    assert_eq!(d.locate(0), Loc { block: 0, block_start: 0 });
    assert_eq!(d.locate(3), Loc { block: 0, block_start: 0 });
    assert_eq!(d.locate(4), Loc { block: 1, block_start: 4 });
    assert_eq!(d.locate(6), Loc { block: 2, block_start: 6 });
}

#[test]
fn text_counts_utf16_units() {
    // "ї" is one unit, the emoji is a surrogate pair (two units).
    let d = doc(vec![para("ї😀z")]);
    assert_eq!(d.content_size(), 2 + 4);
    assert_eq!(d.text_between(1, 2, ""), "ї");
    assert_eq!(d.text_between(2, 4, ""), "😀");
    assert_eq!(d.text_between(4, 5, ""), "z");
    // A cut inside the pair never splits the character or panics.
    assert_eq!(d.text_between(3, 5, ""), "😀z");
    assert_eq!(d.text_between(2, 3, ""), "");
}

#[test]
fn containers_and_leaves_have_their_sizes() {
    let cell = |s: &str| Cell { blocks: vec![para(s)], ..Default::default() };
    let table = Block::Table(Table { col_widths: vec![], rows: vec![Row { cells: vec![cell("a"), cell("bc")] }] });
    // table 2 + row 2 + cell(2 + para 3) + cell(2 + para 4)
    assert_eq!(table.size(), 2 + 2 + 5 + 6);
    let list =
        Block::List(List { ordered: true, style: None, start: 1, items: vec![ListItem { blocks: vec![para("x")] }] });
    assert_eq!(list.size(), 2 + 2 + 3);
    assert_eq!(leaf(LeafKind::Rule).size(), 1);
    let d = doc(vec![leaf(LeafKind::Hidden), list, table, Block::Quote(Quote { blocks: vec![para("q")] })]);
    assert_eq!(d.content_size(), 1 + 7 + 15 + 5);
    assert_eq!(d.text_between(0, d.content_size(), "|"), "x|a|bc|q");
    // The text of the second cell: list 1..8, table at 8, row at 9, first cell 10..15, second cell at 15, para at 16.
    assert_eq!(d.text_between(17, 19, ""), "bc");
}

#[test]
fn atoms_take_one_position_and_no_text() {
    let atom = Inline::Atom(Atom { kind: AtomKind::Math, width: 10.0, height: 8.0, depth: 2.0, data: None });
    let d = doc(vec![Block::Paragraph(Paragraph { inlines: vec![text("a"), atom, text("b")], ..Default::default() })]);
    assert_eq!(d.content_size(), 5);
    assert_eq!(d.text_between(0, 5, ""), "ab");
    assert_eq!(d.text_between(3, 4, ""), "b");
}

#[test]
fn json_is_compact_and_round_trips() {
    let d = doc(vec![para("Привіт")]);
    let json = d.to_json();
    assert!(json.contains(r#"{"type":"paragraph","inlines":[{"type":"text","text":"Привіт"}]}"#), "{json}");
    assert_eq!(Document::from_json(&json), Ok(d));
}

#[test]
fn colours_are_hex_strings() {
    let marks = Marks { color: Some(Color::rgb(255, 0, 16)), ..Default::default() };
    let json = serde_json::to_string(&marks).unwrap();
    assert_eq!(json, r##"{"color":"#ff0010"}"##);
    assert_eq!(Color::parse("#F00"), Some(Color::rgb(255, 0, 0)));
    assert_eq!(Color::parse("red"), None);
    assert_eq!(Color::parse("#ґґґ"), None);
    assert!(serde_json::from_str::<Marks>(r#"{"color":"red"}"#).is_err());
}

#[test]
fn invalid_documents_are_rejected_not_laid_out() {
    assert_eq!(Document::from_json(r#"{"version":99,"blocks":[]}"#), Err(ModelError::UnsupportedVersion(99)));
    assert!(matches!(Document::from_json("{"), Err(ModelError::Json(_))));
    let bad = |json: &str| matches!(Document::from_json(json), Err(ModelError::Invalid(_)));
    assert!(bad(r#"{"version":1,"blocks":[{"type":"paragraph","level":7}]}"#));
    assert!(bad(r#"{"version":1,"blocks":[{"type":"paragraph","props":{"lineHeight":0}}]}"#));
    assert!(bad(r#"{"version":1,"blocks":[{"type":"leaf","kind":"image","width":-1}]}"#));
    assert!(bad(r#"{"version":1,"blocks":[{"type":"table","rows":[{"cells":[{"colspan":0,"blocks":[]}]}]}]}"#));
    assert!(bad(
        r#"{"version":1,"page":{"width":0,"height":10,"marginTop":0,"marginRight":0,"marginBottom":0,"marginLeft":0},"blocks":[]}"#
    ));
    // serde_json has no NaN literal, so non-finite numbers can only arrive through the API.
    let mut d = doc(vec![]);
    let nan = Block::Leaf(Leaf { kind: LeafKind::Image, width: f32::NAN, height: 1.0, data: None });
    assert!(matches!(d.replace_blocks(0, 0, vec![nan]), Err(ModelError::Invalid(_))));
    assert!(d.blocks.is_empty());
}

#[test]
fn nesting_deeper_than_the_limit_is_rejected() {
    let mut b = para("x");
    for _ in 0..MAX_DEPTH + 2 {
        b = Block::Quote(Quote { blocks: vec![b] });
    }
    assert!(matches!(doc(vec![b]).validate(), Err(ModelError::Invalid(_))));
}

#[test]
fn replace_blocks_edits_a_range() {
    let mut d = doc(vec![para("a"), para("b"), para("c")]);
    d.replace_blocks(1, 2, vec![para("x"), para("y")]).unwrap();
    assert_eq!(d.text_between(0, d.content_size(), "|"), "a|x|y|c");
    d.replace_blocks(0, 4, vec![]).unwrap();
    assert_eq!(d.content_size(), 0);
    assert_eq!(d.replace_blocks(1, 0, vec![]), Err(ModelError::BadRange { from: 1, to: 0, len: 0 }));
    assert_eq!(d.replace_blocks(0, 1, vec![]), Err(ModelError::BadRange { from: 0, to: 1, len: 0 }));
}

// ---- property tests -------------------------------------------------------------------------------------------

fn arb_text() -> impl Strategy<Value = String> {
    // Latin, Cyrillic, spaces, a combining mark and an astral character (surrogate pair in UTF-16).
    proptest::collection::vec(
        prop_oneof![Just('a'), Just('Ж'), Just('ї'), Just(' '), Just('\u{301}'), Just('😀'), Just('\t')],
        1..12,
    )
    .prop_map(|v| v.into_iter().collect())
}

fn arb_inline() -> impl Strategy<Value = Inline> {
    prop_oneof![
        4 => (arb_text(), any::<bool>(), any::<bool>()).prop_map(|(t, bold, italic)| Inline::Text(Text { text: t, marks: Marks { bold, italic, ..Default::default() } })),
        1 => (0.0f32..50.0, 0.0f32..30.0).prop_map(|(w, h)| Inline::Atom(Atom { kind: AtomKind::Math, width: w, height: h, depth: 0.0, data: None })),
    ]
}

fn arb_para() -> impl Strategy<Value = Block> {
    (0u8..=3, proptest::collection::vec(arb_inline(), 0..5))
        .prop_map(|(level, inlines)| Block::Paragraph(Paragraph { level, inlines, ..Default::default() }))
}

pub(crate) fn arb_block() -> impl Strategy<Value = Block> {
    let leaf = prop_oneof![Just(LeafKind::Rule), Just(LeafKind::PageBreak), Just(LeafKind::Hidden)].prop_map(leaf);
    prop_oneof![4 => arb_para(), 1 => leaf].prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 1..3).prop_map(|blocks| Block::Quote(Quote { blocks })),
            (any::<bool>(), proptest::collection::vec(proptest::collection::vec(inner.clone(), 1..3), 1..3)).prop_map(
                |(ordered, items)| Block::List(List {
                    ordered,
                    style: None,
                    start: 1,
                    items: items.into_iter().map(|blocks| ListItem { blocks }).collect()
                })
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
    proptest::collection::vec(arb_block(), 0..6).prop_map(doc)
}

proptest! {
    #[test]
    fn any_document_validates_and_round_trips(d in arb_doc()) {
        prop_assert_eq!(d.validate(), Ok(()));
        prop_assert_eq!(Document::from_json(&d.to_json()), Ok(d));
    }

    #[test]
    fn every_text_run_is_found_at_its_position(d in arb_doc()) {
        let mut runs = Vec::new();
        let mut last_end = 0u32;
        d.walk(|e| {
            let (pos, size) = match e {
                Event::Textblock { pos, .. } => (pos, 1),
                Event::Text { pos, text } => { runs.push((pos, text.text.clone())); (pos, utf16_len(&text.text)) }
                Event::Atom { pos, .. } | Event::LeafBlock { pos, .. } => (pos, 1),
            };
            // Events come in position order and never overlap.
            assert!(pos >= last_end, "event at {pos} overlaps the previous one ending at {last_end}");
            last_end = pos + size;
        });
        prop_assert!(last_end <= d.content_size());
        for (pos, text) in runs {
            prop_assert_eq!(d.text_between(pos, pos + utf16_len(&text), ""), text);
        }
    }

    #[test]
    fn slicing_at_any_two_positions_never_panics_and_adds_up(d in arb_doc(), a in 0u32..400, b in 0u32..400) {
        let size = d.content_size();
        let (a, b) = (a.min(size), b.min(size));
        let (a, b) = (a.min(b), a.max(b));
        let whole = d.text_between(0, size, "");
        let parts = d.text_between(0, a, "") + &d.text_between(a, b, "") + &d.text_between(b, size, "");
        // A cut inside a surrogate pair moves the whole character to the right-hand part, so nothing is lost.
        prop_assert_eq!(parts, whole);
    }

    #[test]
    fn locate_agrees_with_block_sizes(d in arb_doc(), pos in 0u32..400) {
        let pos = pos.min(d.content_size());
        let loc = d.locate(pos);
        let start: u32 = d.blocks.iter().take(loc.block).map(Block::size).sum();
        prop_assert_eq!(loc.block_start, start);
        if let Some(b) = d.blocks.get(loc.block) {
            prop_assert!(pos >= start && pos < start + b.size());
        } else {
            prop_assert_eq!(pos, d.content_size());
        }
    }

    #[test]
    fn replacing_blocks_keeps_the_document_valid(d in arb_doc(), new in proptest::collection::vec(arb_block(), 0..3), a in 0usize..8, b in 0usize..8) {
        let mut d = d;
        let (from, to) = (a.min(b).min(d.blocks.len()), a.max(b).min(d.blocks.len()));
        let before: u32 = d.blocks.iter().take(from).map(Block::size).sum();
        let removed: u32 = d.blocks.iter().skip(from).take(to - from).map(Block::size).sum();
        let added: u32 = new.iter().map(Block::size).sum();
        let size = d.content_size();
        prop_assert_eq!(d.replace_blocks(from, to, new), Ok(()));
        prop_assert_eq!(d.content_size(), size - removed + added);
        prop_assert_eq!(d.locate(before).block_start, before);
        prop_assert_eq!(d.validate(), Ok(()));
    }
}
