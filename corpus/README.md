# Document corpus

Every folder is one document in the engine's format (`doc.json`). The corpus feeds the reference snapshots (L3,
`crates/cli/tests/corpus.rs`): every document is read, described and laid out, and the results are pinned as
`insta` snapshots. The `corpus_is_not_empty` test requires at least 4 documents.

| Folder | What it checks |
|---|---|
| `empty` | an empty document |
| `one-char` | a single character |
| `plain` | typical report text: headings, justified paragraphs with a first-line indent |
| `marks` | text formatting: styles, sizes, colours, subscripts and superscripts, fonts, an inline formula and footnote |
| `structure` | lists, a nested list, a table with a merged cell, a picture, a formula, a quote, code |
