# Layout engine architecture

The engine turns a report document into pages: lines, their coordinates, a list of what to paint, and a PDF.
It knows nothing about the editor, the browser or ProseMirror: document JSON and font bytes go in, numbers come
out. The same code runs in the browser (WebAssembly) and natively (the `sfdoc` CLI), so pagination is the same
everywhere.

## Layers

| Layer | Crate | Job |
|---|---|---|
| 0 | `sfdoc-geom` | units (pt), points, rectangles |
| 0 | `sfdoc-spell` | spell checking (see below); depends on no layout crate |
| 1 | `sfdoc-model` | document format, positions, block replacement |
| 1 | `sfdoc-fonts` | fonts: metrics, shaping (rustybuzz), fallback font per character, outlines |
| 2 | `sfdoc-layout` | paragraph to lines, page flow, tables, footnotes, contents, geometry queries, display list |
| 3 | `sfdoc-raster` | page PNG (tests and the CLI) |
| 3 | `sfdoc-pdf` | PDF from the display list (krilla) |
| 4 | `sfdoc-wasm` | JavaScript bindings of the layout engine |
| 4 | `sfdoc-spell-wasm` | JavaScript bindings of the speller, a separate wasm module |
| 4 | `sfdoc-cli` | the `sfdoc` CLI: `inspect`, `layout`, `render`, `pdf`, `bench` |

A crate depends only on lower layers; `cargo xtask layers` checks this (the list is `LAYERS` in
`xtask/src/main.rs`). `unsafe` is forbidden throughout, and clippy rejects `unwrap`, `expect`, `panic!` and
unchecked indexing: the engine must not crash on any document. The exception is `sfdoc-wasm` (wasm-bindgen
expands to `unsafe`), so its logic lives in a plain `engine.rs` that is tested natively. The same goes for
`sfdoc-spell-wasm`: it has no logic of its own and only forwards calls to `sfdoc-spell`.

## Document and positions

`sfdoc-model`: `Document { version, page, running, text, styles, blocks }`. Blocks are paragraph, heading, list,
table, quote, code, and leaf (picture, formula, page break, contents). Inside a paragraph there is formatted
text, atoms (inline formula, picture, footnote mark) and forced line breaks.

A position is one integer in the "flat token" scheme: entering and leaving a node count 1 each, an atom counts
1, text counts in UTF-16 code units. The scheme deliberately matches ProseMirror's positions: an editor built on
the same scheme shares positions with the engine without translation. The format is versioned
(`FORMAT_VERSION`); `replace_blocks(from, to, json)` replaces a range of blocks without sending the whole
document.

## Layout

1. **Paragraph** (`para.rs`). Text is cut at UAX #14 line-break opportunities and at changes of formatting or
   font; every piece is shaped on its own, and lines are filled greedily. Justification stretches the spaces.
   Line spacing works as in Word (a multiple of the font's line height).
2. **Flow** (`flow.rs`). Blocks are laid out on pages: "keep with next" for headings, widow and orphan control,
   forced breaks, headers, footers and the page number. A picture or formula taller than a page is scaled down
   to the page height.
3. **Tables**. Column widths are given or come from the content (the longest word is not broken). A row moves to
   the next page whole; a row taller than a page continues line by line; header rows repeat (they are only
   painted and have no positions). A table inside a cell moves to the next page row by row together with the cell
   that holds it. Grid lines: all, between rows only, frame only, or none (`borders`).
4. **Footnotes** go to the bottom of the page where their mark stands; a line moves on together with its
   footnote.
5. **Contents**: the engine builds it from the document's headings, with real page numbers. The same goes for a
   list of figures or tables: a contents block with `list` shows the paragraphs marked with `entryOf` of the same
   name.
6. **Document styles** (`styles` in the document: `body`, `h1`…`h6`, `code` and any name a paragraph states in
   `style`) change the default look of their kind of paragraph; the paragraph's own formatting and the text's
   formatting win over them. The body text font is the font of the whole document, including the contents,
   headers, footers and page numbers.
7. **A picture that text flows around** (`wrap` plus left or right alignment) has no place of its own in the
   flow: the next paragraph carries it and puts it beside its first line. Lines that start above the bottom of the
   picture are shorter by its width; a paragraph that ends earlier hands the remaining height to the next one.
   What does not flow around it (a table, a section heading that starts a new page, another picture) goes below
   the picture. A picture with no paragraph after it, wider than 60 % of the text width or taller than 60 % of the
   page stands alone, like an ordinary one.
8. **Hyphenation** (`text.hyphenate`): a word that does not fit on the line is split at a syllable boundary
   (`hyphen.rs`, rules for Ukrainian and Russian, no dictionaries). The hyphen is only painted and has no
   position.
9. **Even-page headers and footers and the watermark** are in `running`; the watermark is painted under
   everything else.
10. **Formatting marks** (`page_with(.., marks)`) are for the screen only: dots in spaces, arrows for tabs and
    forced breaks, ¶ at the end of a paragraph. They never reach the PDF.

Pages are recomputed in full after every edit; only the breaking of a block into lines is cached (an edit drops
the cache only for the blocks it replaced). So layout after an edit equals a full layout by construction. This is
the main invariant, and the `incremental` property test checks it.

`fingerprint(page)` is a hash of what is painted on a page, without positions: a letter typed on page three
shifts the positions further down the document but not the look of page ten, so page ten need not be repainted.

## Geometry queries

`caret_rect(pos)`, `hit_test(page, x, y)`, `selection_rects(from, to)`, `line_move(pos, down, goal_x)`,
`line_bounds(pos)`, `page_of(pos)`. Editor input rests on them: the caret, selection, the mouse, moving by
lines. The order of items on a page does not match the order of positions (a table row split across pages), so
the queries do not rely on ordering.

## Display list

`page(index)` returns items of three kinds: `Glyphs` (font, size, colour, origin, glyphs with offsets, and the
text), `Rect` (table rules, underlines, backgrounds), `Object` (a picture or formula: a rectangle and a key).
Every painter draws from the same list: the browser (DOM, one element per word), the rasterizer (PNG for tests)
and the PDF writer.

What a painter that draws a page in the browser must do so that widths match the engine:

- Draw every `Glyphs` run (a word or part of one) as its own element at the origin the engine gave. A line drawn
  as one string drifts from the engine by up to 2 px, because browsers kern around spaces differently.
- Draw text with `text-rendering: geometricPrecision`. Otherwise Chromium rounds glyph widths to whole pixels and
  a single word drifts from the engine by several pixels; with it, the difference stays under 0.2 px in Chromium,
  Firefox and WebKit.

## PDF

`sfdoc-pdf` puts the same glyphs at the same coordinates. Text can be selected and searched (glyphs map to text
through `cluster`), fonts are embedded as subsets, links become annotations. Formulas arrive as SVG from MathJax;
they are read by an SVG reader of our own that understands outlines only (`outline_svg.rs`): a full SVG renderer
weighs more than the rest of the engine and is enabled only in the CLI (the `svg` feature). The caller turns the
text inside a formula into outlines with the engine's font beforehand (`text_outline`). Whatever cannot be drawn
becomes an empty frame and goes into the `missing` list.

## Fonts

The engine has no built-in fonts and reads no files: whoever runs it passes the bytes. A font's number in the
display list is its number in load order, so the browser and the engine must load the same files in the same
order. The set is Liberation (Serif, Sans, Mono), metric-compatible with Times New Roman, Arial and Courier New.

## Spell checking

`sfdoc-spell` stands apart from layout: it depends on no engine crate and is built into a separate wasm module
(`sfdoc-spell-wasm`), so an editor can load it only when spell checking is on. Dictionaries are Hunspell files
read by `spellbook`; like fonts, their bytes are passed in by whoever runs it. The crate adds what an editor needs
around the word lookup: it cuts a paragraph into words, skips addresses, file names, abbreviations and
identifiers, chooses the dictionary by the script of the word (Cyrillic or Latin), flags a word that mixes
letters of two scripts (a Latin "e" in a Ukrainian word) and offers its fix first, and keeps the user's own
words. Offsets are in UTF-16 code units, like text positions in the browser.

## Tests and budgets

| Level | What | Where |
|---|---|---|
| L0 | fmt, clippy, layers | `cargo xtask ci` |
| L1 | unit | next to the code: `crates/*/src/tests.rs` or inline test modules |
| L2 | properties: nothing leaves the page, order, hit-test and caret agree, layout after edits equals a full layout | `crates/layout/src/tests.rs` (proptest) |
| L3 | snapshots: where every line of the corpus landed | `crates/cli/tests/snapshots` (insta) |
| L7 | speed and size | `sfdoc bench [pages] [budget, ms]`, wasm at most 1.5 MB gzip |

Levels L4 (comparison with LibreOffice), L5 (PDF without a browser) and L6 (three browsers) need an editor, so
they run outside this repo, in the editor app that embeds the engine.

## Packages and releases

The engine ships as two npm packages: `@studying-skills/docengine` (`sfdoc-wasm`) and
`@studying-skills/spellengine` (`sfdoc-spell-wasm`). A `v*` tag runs `.github/workflows/publish.yml`: it builds
both wasm modules, assembles the packages (`scripts/pack-npm.sh`) and publishes both with one version, which must
match the workspace version in `Cargo.toml`. A pull request that touches packaging builds the packages without
publishing them.
