//! L3: every corpus document is read, described and laid out; the results are pinned as snapshots.
//! A snapshot changes only in a commit that says what changed in the layout and why.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn corpus() -> Vec<(String, PathBuf)> {
    let mut docs: Vec<_> = std::fs::read_dir(root().join("corpus"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("doc.json").exists())
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                p.join("doc.json"),
            )
        })
        .collect();
    docs.sort();
    docs
}

#[test]
fn corpus_is_not_empty() {
    assert!(corpus().len() >= 4);
}

#[test]
fn inspect_snapshots() {
    for (name, path) in corpus() {
        let doc = sfdoc_cli::read_doc(&path).unwrap();
        insta::assert_snapshot!(format!("inspect__{name}"), sfdoc_cli::inspect(&doc));
    }
}

#[test]
fn layout_snapshots() {
    let fonts = sfdoc_cli::load_fonts(&root().join("assets/fonts")).unwrap();
    for (name, path) in corpus() {
        let doc = sfdoc_cli::read_doc(&path).unwrap();
        let layout = sfdoc_layout::layout(&doc, &fonts).unwrap();
        assert!(!layout.pages.is_empty(), "{name}: a document always has a page");
        insta::assert_json_snapshot!(format!("layout__{name}"), layout);
    }
}

#[test]
fn render_writes_a_png_and_reports_bad_arguments() {
    let dir = std::env::temp_dir().join(format!("sfdoc-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("page.png");
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let doc = root().join("corpus/plain/doc.json");
    let fonts = root().join("assets/fonts");
    let (doc, fonts, out_s) = (doc.to_str().unwrap(), fonts.to_str().unwrap(), out.to_str().unwrap());
    let msg = sfdoc_cli::run(&s(&["render", doc, "-o", out_s, "--fonts", fonts])).unwrap();
    assert!(msg.starts_with("page 1 of 1"), "{msg}");
    assert!(std::fs::read(&out).unwrap().starts_with(b"\x89PNG"));
    assert!(
        sfdoc_cli::run(&s(&["render", doc, "-o", out_s, "--fonts", fonts, "--page", "9"]))
            .unwrap_err()
            .contains("page 9 was asked for")
    );
    assert!(
        sfdoc_cli::run(&s(&["render", doc, "--fonts", fonts]))
            .unwrap_err()
            .contains("-o")
    );
    assert!(
        sfdoc_cli::run(&s(&["render", doc, "-o", out_s, "--fonts", "/nonexistent"]))
            .unwrap_err()
            .contains("cannot read fonts")
    );
    assert!(sfdoc_cli::run(&s(&["inspect", "/nonexistent.json"])).is_err());
    assert!(
        sfdoc_cli::run(&s(&["frobnicate"]))
            .unwrap_err()
            .contains("unknown command")
    );
    assert!(sfdoc_cli::run(&[]).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}
