use super::*;

fn load(style: &str) -> Font {
    let path = format!(
        "{}/../../assets/fonts/liberation/LiberationSerif-{style}.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    Font::from_bytes(std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn garbage_is_an_error_not_a_panic() {
    assert_eq!(Font::from_bytes(vec![]).unwrap_err(), FontError::Unreadable);
    assert_eq!(
        Font::from_bytes(b"not a font at all".to_vec()).unwrap_err(),
        FontError::Unreadable
    );
    let mut truncated = std::fs::read(format!(
        "{}/../../assets/fonts/liberation/LiberationSerif-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    truncated.truncate(200);
    assert!(Font::from_bytes(truncated).is_err());
}

#[test]
fn liberation_serif_has_the_metrics_of_times_new_roman() {
    let m = *load("Regular").metrics();
    assert_eq!(m.units_per_em, 2048.0);
    // Times New Roman: ascent 1825, descent 443, line gap 87 (hhea) — the numbers Word's line spacing comes from.
    assert_eq!((m.ascent, m.descent, m.line_gap), (1825.0, 443.0, 87.0));
    // Single line spacing at 14 pt = 16.1 pt, so "1.5" is 24.15 pt.
    assert!((m.line_height(14.0) - 16.099).abs() < 0.01, "{}", m.line_height(14.0));
    assert!(m.x_height > 800.0 && m.x_height < 1000.0);
    assert!(m.underline_offset > 0.0 && m.underline_thickness > 0.0);
}

#[test]
fn ukrainian_text_shapes_one_glyph_per_letter_with_clusters() {
    let font = load("Regular");
    let s = font.shaper().unwrap();
    let glyphs = s.shape("Її Єґ");
    assert_eq!(glyphs.len(), 5);
    assert!(
        glyphs.iter().all(|g| g.id != 0),
        "a letter fell back to .notdef: {glyphs:?}"
    );
    // Clusters are byte offsets: Cyrillic letters are two bytes each, the space one.
    assert_eq!(
        glyphs.iter().map(|g| g.cluster).collect::<Vec<_>>(),
        vec![0, 2, 4, 5, 7]
    );
    assert!(s.shape("").is_empty());
}

#[test]
fn widths_match_what_browsers_measured_in_the_spike() {
    // E0/S4: Chromium laid out "Привіт, світе" at 18.666666 px with this font file as 101.6185 px wide.
    let font = load("Regular");
    let s = font.shaper().unwrap();
    assert!((s.advance("Привіт, світе", 18.666_666) - 101.6185).abs() < 0.01);
}

#[test]
fn kerning_is_applied() {
    let font = load("Regular");
    let s = font.shaper().unwrap();
    let together = s.advance("AV", 100.0);
    let apart = s.advance("A", 100.0) + s.advance("V", 100.0);
    assert!(together < apart - 1.0, "AV {together} vs A+V {apart}");
}

#[test]
fn coverage_and_outlines() {
    let font = load("Regular");
    let s = font.shaper().unwrap();
    assert!(s.covers('ґ') && s.covers('A') && s.covers('№'));
    assert!(!s.covers('日') && !s.covers('😀'));

    #[derive(Default)]
    struct Count(usize, usize);
    impl OutlineSink for Count {
        fn move_to(&mut self, _: f32, _: f32) {
            self.0 += 1;
        }
        fn line_to(&mut self, _: f32, _: f32) {
            self.1 += 1;
        }
        fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
            self.1 += 1;
        }
        fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
            self.1 += 1;
        }
        fn close(&mut self) {}
    }
    let o = s.shape("о")[0].id;
    let mut c = Count::default();
    assert!(s.outline(o, &mut c));
    // The letter "о" is two contours: the outside and the counter.
    assert_eq!(c.0, 2);
    assert!(c.1 > 8);
    let space = s.shape(" ")[0].id;
    assert!(!s.outline(space, &mut Count::default()));
}

#[test]
fn font_set_resolves_word_names_styles_and_fallbacks() {
    let mut set = FontSet::new();
    assert_eq!(set.resolve(Some("Arial"), false, false), None);
    let regular = set.add("Liberation Serif", false, false, load("Regular"));
    let bold = set.add("Liberation Serif", true, false, load("Bold"));
    let italic = set.add("Liberation Serif", false, true, load("Italic"));

    let r = |f: Option<&str>, b, i| set.resolve(f, b, i).unwrap();
    assert_eq!(r(Some("Liberation Serif"), false, false).id, regular);
    assert_eq!(
        r(Some("Times New Roman"), true, false),
        Resolved {
            id: bold,
            missing_bold: false,
            missing_italic: false
        }
    );
    assert_eq!(r(Some("'Times New Roman', serif"), false, true).id, italic);
    // No bold italic face was added: bold wins and the italic is reported missing.
    assert_eq!(
        r(Some("times new roman"), true, true),
        Resolved {
            id: bold,
            missing_bold: false,
            missing_italic: true
        }
    );
    // An unknown family and no family at all fall back to the default family, keeping the style.
    assert_eq!(r(Some("Comic Sans MS"), true, false).id, bold);
    assert_eq!(r(None, false, false).id, regular);
    // Arial's substitute is not loaded: the default family again.
    assert_eq!(r(Some("Arial"), false, false).id, regular);
    assert_eq!(set.len(), 3);
    assert!(set.get(bold).is_some() && set.get(FontId(9)).is_none());
}

#[test]
fn a_font_file_describes_itself() {
    let d = |s| load(s).describe().unwrap();
    assert_eq!(
        d("Regular"),
        Description {
            family: "Liberation Serif".into(),
            bold: false,
            italic: false
        }
    );
    assert_eq!(
        d("BoldItalic"),
        Description {
            family: "Liberation Serif".into(),
            bold: true,
            italic: true
        }
    );
    let mut set = FontSet::new();
    let (id, _) = set.add_described(load("Italic")).unwrap();
    assert_eq!(set.resolve(Some("Times New Roman"), false, true).unwrap().id, id);
}
