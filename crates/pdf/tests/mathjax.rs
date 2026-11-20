//! Real MathJax output (30 formulas) through the outline reader.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

#[test]
fn mathjax_formulas_are_read_as_outlines() {
    let text = std::fs::read_to_string(format!("{}/tests/mathjax.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let all: serde_json::Value = serde_json::from_str(&text).unwrap();
    let mut refused = Vec::new();
    for f in all.as_array().unwrap() {
        let (latex, svg) = (f["latex"].as_str().unwrap(), f["svg"].as_str().unwrap());
        match sfdoc_pdf::outline_svg::read(svg) {
            Some(d) => {
                assert!(!d.shapes.is_empty(), "{latex}: nothing drawn");
                // Every outline stays near the view box: a wrong transform would throw it far away.
                let [x, y, w, h] = d.view;
                for s in d.shapes.iter().flat_map(|s| &s.segs) {
                    use sfdoc_pdf::outline_svg::Seg;
                    let (px, py) = match *s {
                        Seg::Move(a, b) | Seg::Line(a, b) | Seg::Quad(_, _, a, b) | Seg::Cubic(_, _, _, _, a, b) => {
                            (a, b)
                        }
                        Seg::Close => continue,
                    };
                    let m = 0.15 * w.max(h);
                    assert!(
                        px >= x - m && px <= x + w + m && py >= y - m && py <= y + h + m,
                        "{latex}: point {px},{py} outside {:?}",
                        d.view
                    );
                }
            }
            None => refused.push(latex.to_owned()),
        }
    }
    // Formulas with \text{кирилиця} carry <text> elements: the reader refuses them until the engine outlines
    // that text itself.
    assert_eq!(
        refused,
        vec![
            r"v_{\text{сер}} = \frac{s_{\text{заг}}}{t}",
            r"\eta = \frac{A_{\text{кор}}}{A_{\text{повна}}} \cdot 100\%"
        ]
    );
}
