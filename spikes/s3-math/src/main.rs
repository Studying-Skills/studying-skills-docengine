//! Spike S3: MathJax SVG formulas drawn into a PDF as vectors, at the boxes from layout.json.
use std::sync::Arc;

use krilla::Document;
use krilla::geom::{Point, Size, Transform};
use krilla::page::PageSettings;
use krilla::text::{Font, TextDirection};
use krilla_svg::{SurfaceExt, SvgSettings};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let items: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let font_data = std::fs::read(&a[2]).unwrap();
    let font = Font::new(Arc::new(font_data.clone()).into(), 0).unwrap();
    let mut opt = usvg::Options::default();
    Arc::make_mut(&mut opt.fontdb).load_font_data(font_data);
    opt.font_family = "Liberation Serif".into();
    let items = items.as_array().unwrap();
    let pages = items.iter().map(|i| i["page"].as_u64().unwrap()).max().unwrap() + 1;
    let mut doc = Document::new();
    for p in 0..pages {
        let mut page = doc.start_page_with(PageSettings::from_wh(595.276, 841.89).unwrap());
        let mut s = page.surface();
        for it in items.iter().filter(|i| i["page"].as_u64() == Some(p)) {
            let f = |k: &str| it[k].as_f64().unwrap() as f32;
            s.draw_text(Point::from_xy(60.0, f("baseline")), font.clone(), 14.0, &format!("({})", it["i"].as_u64().unwrap() + 1), false, TextDirection::Auto);
            let tree = usvg::Tree::from_str(it["svg"].as_str().unwrap(), &opt).unwrap();
            s.push_transform(&Transform::from_translate(f("x"), f("top")));
            s.draw_svg(&tree, Size::from_wh(f("w"), f("h")).unwrap(), SvgSettings { embed_text: false, ..Default::default() });
            s.pop();
        }
        s.finish();
        page.finish();
    }
    std::fs::write(&a[3], doc.finish().unwrap()).unwrap();
}
