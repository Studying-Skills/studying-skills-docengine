#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use sfdoc_fonts::Font;
use sfdoc_model::{Block, Document, Inline, Leaf, LeafKind, Marks, Paragraph, Text};

use super::{FontSet, HashMap, Info, NoAssets, Picture, write_pdf};

fn fonts() -> FontSet {
    let mut set = FontSet::new();
    for name in ["Serif-Regular", "Serif-Bold"] {
        let path = format!("{}/../../assets/fonts/liberation/Liberation{name}.ttf", env!("CARGO_MANIFEST_DIR"));
        set.add_described(Font::from_bytes(std::fs::read(path).unwrap()).unwrap()).unwrap();
    }
    set
}

fn para(s: &str) -> Block {
    Block::Paragraph(Paragraph {
        inlines: vec![Inline::Text(Text { text: s.into(), marks: Marks::default() })],
        ..Default::default()
    })
}

fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

#[test]
fn every_layout_page_becomes_a_pdf_page() {
    let f = fonts();
    let blocks: Vec<Block> = (0..70).map(|i| para(&format!("Рядок {i}"))).collect();
    let doc = Document { blocks, ..Default::default() };
    let layout = sfdoc_layout::layout(&doc, &f).unwrap();
    assert_eq!(layout.pages.len(), 3);
    let (pdf, report) =
        write_pdf(&layout, &f, &NoAssets, &Info { title: Some("Звіт".into()), readable: true, ..Default::default() })
            .unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    assert!(report.missing.is_empty());
    assert_eq!(
        count(&pdf, b"/Type /Page\n")
            + count(&pdf, b"/Type /Page ")
            + count(&pdf, b"/Type /Page>")
            + count(&pdf, b"/Type /Page/"),
        3
    );
    // The same input gives the same bytes: exports can be compared and cached.
    assert_eq!(
        pdf,
        write_pdf(&layout, &f, &NoAssets, &Info { title: Some("Звіт".into()), readable: true, ..Default::default() })
            .unwrap()
            .0
    );
}

#[test]
fn a_missing_picture_is_reported_and_does_not_stop_the_export() {
    let f = fonts();
    let image = Block::Leaf(Leaf {
        kind: LeafKind::Image,
        width: 100.0,
        height: 50.0,
        data: Some("figs/a.png".into()),
        align: None,
        label: None,
        list: None,
        wrap: false,
    });
    let doc = Document { blocks: vec![para("до"), image.clone(), image], ..Default::default() };
    let layout = sfdoc_layout::layout(&doc, &f).unwrap();
    let (_, report) = write_pdf(&layout, &f, &NoAssets, &Info { readable: true, ..Default::default() }).unwrap();
    assert_eq!(report.missing, vec!["figs/a.png", "figs/a.png"]);

    // A real PNG and an SVG are drawn; bytes that are no picture are reported.
    let mut assets: HashMap<String, Picture> = HashMap::new();
    let mut pm = Vec::new();
    {
        // 1×1 white PNG.
        pm.extend_from_slice(&[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0,
            1, 8, 2, 0, 0, 0, 0x90, 0x77, 0x53, 0xde, 0, 0, 0, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0xf8,
            0xff, 0xff, 0x3f, 0, 0x05, 0xfe, 0x02, 0xfe, 0xdc, 0xcc, 0x59, 0xe7, 0, 0, 0, 0, 0x49, 0x45, 0x4e, 0x44,
            0xae, 0x42, 0x60, 0x82,
        ]);
    }
    assets.insert("figs/a.png".into(), Picture::Raster(pm));
    let (with, report) = write_pdf(&layout, &f, &assets, &Info { readable: true, ..Default::default() }).unwrap();
    assert!(report.missing.is_empty(), "{report:?}");
    assert!(count(&with, b"/Subtype /Image") >= 1);
    assets.insert(
        "figs/a.png".into(),
        Picture::Svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 5"><path d="M0 0h10v5z"/></svg>"#.into(),
        ),
    );
    assert!(
        write_pdf(&layout, &f, &assets, &Info { readable: true, ..Default::default() }).unwrap().1.missing.is_empty()
    );
    assets.insert("figs/a.png".into(), Picture::Raster(vec![1, 2, 3, 4, 5]));
    assert_eq!(
        write_pdf(&layout, &f, &assets, &Info { readable: true, ..Default::default() }).unwrap().1.missing.len(),
        2
    );
}

#[test]
fn links_become_annotations() {
    let f = fonts();
    let link = Inline::Text(Text {
        text: "сайт".into(),
        marks: Marks { link: Some("https://example.com/x".into()), ..Default::default() },
    });
    let doc = Document {
        blocks: vec![Block::Paragraph(Paragraph { inlines: vec![link], ..Default::default() })],
        ..Default::default()
    };
    let layout = sfdoc_layout::layout(&doc, &f).unwrap();
    let (pdf, _) = write_pdf(&layout, &f, &NoAssets, &Info { readable: true, ..Default::default() }).unwrap();
    assert_eq!(count(&pdf, b"https://example.com/x"), 1);
    assert_eq!(count(&pdf, b"/Subtype /Link"), 1);
}
