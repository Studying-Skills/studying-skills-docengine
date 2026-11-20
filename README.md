# studying-skills-docengine

The layout engine of the StudyFlow report editor: document to lines to pages to PDF. Rust, built natively and to
WebAssembly.

| Layer | Crate | Job |
|---|---|---|
| 0 | `sfdoc-geom` | units and rectangles |
| 0 | `sfdoc-spell` | spell checking: the words of a paragraph that are not in the dictionary, and fixes to offer |
| 1 | `sfdoc-model` | document format, positions, block replacement |
| 1 | `sfdoc-fonts` | fonts: metrics, shaping, fallback fonts |
| 2 | `sfdoc-layout` | layout and the display list |
| 3 | `sfdoc-raster` | page PNG (tests and the CLI) |
| 3 | `sfdoc-pdf` | PDF from the display list |
| 4 | `sfdoc-wasm` | JavaScript bindings of the layout engine |
| 4 | `sfdoc-spell-wasm` | JavaScript bindings of the speller (a separate wasm module) |
| 4 | `sfdoc-cli` | the `sfdoc` CLI |

A lower layer never depends on a higher one: `cargo xtask layers`.

```
cargo test --workspace          # L1–L3
cargo xtask layers              # layers
cargo xtask ci                  # fmt, clippy, tests, layers, build of both wasm modules, wasm size
cargo xtask wasm-size           # size of the already built layout engine wasm against its budget
cargo run -p sfdoc-cli -- inspect corpus/plain/doc.json
cargo run -p sfdoc-cli -- render corpus/plain/doc.json -o page.png
```

Packages: `@studying-skills/docengine` (the layout engine) and `@studying-skills/spellengine` (the speller). Both
are published together, with one version, from a `v*` tag (`.github/workflows/publish.yml`).

Architecture: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
