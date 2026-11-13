//! The `sfdoc` commands as functions, so tests call them without spawning a process.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use sfdoc_fonts::{Font, FontSet};
use sfdoc_model::{Block, Document, Event};

pub const USAGE: &str = "\
sfdoc — StudyFlow document engine

  sfdoc inspect <doc.json>
      print the document's structure: blocks, positions, text
  sfdoc render <doc.json> -o <out.png> [--page N] [--scale S] [--fonts DIR]
      lay the document out and write one page as PNG (page 1, scale 2 = 144 dpi by default)
  sfdoc layout <doc.json> [--fonts DIR]
      lay the document out and print the display list as JSON

Fonts are read from --fonts, else $SFDOC_FONTS, else ./assets/fonts (every .ttf and .otf below it).";

/// Every `.ttf`/`.otf` under `dir`, registered under the family and style its file states. Sorted by path, so the
/// default family does not depend on directory order.
pub fn load_fonts(dir: &Path) -> Result<FontSet, String> {
    fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                collect(&path, out)?;
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("otf"))
            {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    collect(dir, &mut files).map_err(|e| format!("cannot read fonts from {}: {e}", dir.display()))?;
    files.sort();
    let mut set = FontSet::new();
    // Regular faces first: the first family added is the fallback for unknown names.
    let mut fonts = Vec::new();
    for path in files {
        let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let font = Font::from_bytes(data).map_err(|e| format!("{}: {e}", path.display()))?;
        let d = font
            .describe()
            .ok_or_else(|| format!("{}: the font has no family name", path.display()))?;
        fonts.push((d, font));
    }
    fonts.sort_by_key(|(d, _)| (d.bold, d.italic));
    for (d, font) in fonts {
        set.add(&d.family, d.bold, d.italic, font);
    }
    if set.is_empty() {
        return Err(format!("no fonts found in {}", dir.display()));
    }
    Ok(set)
}

pub fn read_doc(path: &Path) -> Result<Document, String> {
    let json = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Document::from_json(&json).map_err(|e| format!("{}: {e}", path.display()))
}

fn kind(b: &Block) -> String {
    match b {
        Block::Paragraph(p) if p.level > 0 => format!("heading {}", p.level),
        Block::Paragraph(_) => "paragraph".into(),
        Block::List(l) => format!(
            "{} list, {} items",
            if l.ordered { "ordered" } else { "bullet" },
            l.items.len()
        ),
        Block::Quote(_) => "quote".into(),
        Block::Table(t) => format!("table, {} rows", t.rows.len()),
        Block::Leaf(l) => format!("{:?}", l.kind).to_lowercase(),
    }
}

/// Human-readable structure of a document: one line per top-level block with its position range and text.
pub fn inspect(doc: &Document) -> String {
    let mut out = String::new();
    let p = &doc.page;
    let mm = sfdoc_geom_mm;
    let _ = writeln!(
        out,
        "format {} · page {:.0}×{:.0} mm · margins top {:.0} right {:.0} bottom {:.0} left {:.0} mm",
        doc.version,
        mm(p.width),
        mm(p.height),
        mm(p.margin_top),
        mm(p.margin_right),
        mm(p.margin_bottom),
        mm(p.margin_left)
    );
    let (mut textblocks, mut atoms, mut leaves, mut chars) = (0, 0, 0, 0u32);
    doc.walk(|e| match e {
        Event::Textblock { .. } => textblocks += 1,
        Event::Text { text, .. } => chars += sfdoc_model::utf16_len(&text.text),
        Event::Atom { .. } => atoms += 1,
        Event::LeafBlock { .. } => leaves += 1,
    });
    let _ = writeln!(
        out,
        "{} top-level blocks · size {} · {textblocks} text blocks · {chars} UTF-16 units of text · {atoms} inline atoms · {leaves} leaf blocks",
        doc.blocks.len(),
        doc.content_size()
    );
    let mut pos = 0u32;
    for (i, b) in doc.blocks.iter().enumerate() {
        let end = pos + b.size();
        let text = doc.text_between(pos, end, " ¶ ");
        // Control characters would break the one-line-per-block listing.
        let shown: String = text
            .chars()
            .take(60)
            .map(|c| match c {
                '\n' => '⏎',
                '\t' => '→',
                c => c,
            })
            .collect();
        let more = if text.chars().count() > 60 { "…" } else { "" };
        let _ = writeln!(out, "{i:>3}  {pos:>5}..{end:<5}  {:<24} {shown}{more}", kind(b));
        pos = end;
    }
    out
}

fn sfdoc_geom_mm(pt: f32) -> f32 {
    pt * 25.4 / 72.0
}

struct Args {
    positional: Vec<String>,
    out: Option<PathBuf>,
    page: usize,
    scale: f32,
    fonts: Option<PathBuf>,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args {
        positional: Vec::new(),
        out: None,
        page: 1,
        scale: 2.0,
        fonts: None,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "-o" | "--out" => a.out = Some(value("-o")?.into()),
            "--fonts" => a.fonts = Some(value("--fonts")?.into()),
            "--page" => {
                a.page = value("--page")?
                    .parse()
                    .ok()
                    .filter(|n| *n >= 1)
                    .ok_or("--page takes a number from 1")?
            }
            "--scale" => {
                a.scale = value("--scale")?
                    .parse()
                    .ok()
                    .filter(|s: &f32| s.is_finite() && *s > 0.0)
                    .ok_or("--scale takes a positive number")?
            }
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s => a.positional.push(s.to_owned()),
        }
    }
    Ok(a)
}

fn fonts_dir(a: &Args) -> PathBuf {
    a.fonts
        .clone()
        .or_else(|| std::env::var_os("SFDOC_FONTS").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("assets/fonts"))
}

/// Run a command line (without the program name). `Ok` is what goes to stdout.
pub fn run(args: &[String]) -> Result<String, String> {
    let Some((cmd, rest)) = args.split_first() else {
        return Err(USAGE.to_owned());
    };
    let a = parse(rest)?;
    let doc_path = || {
        a.positional
            .first()
            .map(PathBuf::from)
            .ok_or_else(|| format!("{cmd}: which document?\n\n{USAGE}"))
    };
    match cmd.as_str() {
        "inspect" => Ok(inspect(&read_doc(&doc_path()?)?)),
        "layout" => {
            let doc = read_doc(&doc_path()?)?;
            let layout = sfdoc_layout::layout(&doc, &load_fonts(&fonts_dir(&a))?).map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&layout).map_err(|e| e.to_string())
        }
        "render" => {
            let doc = read_doc(&doc_path()?)?;
            let out = a.out.clone().ok_or("render: -o <out.png> is required")?;
            let fonts = load_fonts(&fonts_dir(&a))?;
            let layout = sfdoc_layout::layout(&doc, &fonts).map_err(|e| e.to_string())?;
            let page = layout.pages.get(a.page - 1).ok_or_else(|| {
                format!(
                    "the document has {} pages, page {} was asked for",
                    layout.pages.len(),
                    a.page
                )
            })?;
            let png = sfdoc_raster::render_png(page, &fonts, a.scale).map_err(|e| e.to_string())?;
            std::fs::write(&out, &png).map_err(|e| format!("{}: {e}", out.display()))?;
            Ok(format!(
                "page {} of {} → {} ({} bytes)\n",
                a.page,
                layout.pages.len(),
                out.display(),
                png.len()
            ))
        }
        "help" | "--help" | "-h" => Ok(format!("{USAGE}\n")),
        other => Err(format!("unknown command {other}\n\n{USAGE}")),
    }
}
