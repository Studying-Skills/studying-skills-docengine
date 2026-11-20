# Docengine

A report layout engine: document to lines to pages to PDF. Rust, built natively and to WebAssembly.

## Build and test

- `cargo test --workspace`: unit, property and snapshot tests.
- `cargo xtask ci`: what CI runs (fmt, clippy, tests, crate layers, the wasm build and its size budget). It needs `wasm-pack` and the `wasm32-unknown-unknown` target.
- `cargo xtask layers`: checks that a crate depends only on lower layers.
- `cargo run --release -p sfdoc-cli -- bench 50 8 --fonts assets/fonts`: the speed budget CI enforces.
- `node scripts/check-language.mjs`: the language check (see below).
- Snapshots are `insta` files; CI runs with `INSTA_UPDATE=no`. Review a changed snapshot before accepting it.

## Language rule

- Code, comments, doc comments, internal logs and errors, test names and commit messages are in English, even when the user talks to you in another language.
- Program text for developers (CLI help, error messages) is English too.
- Kept as is: corpus documents, snapshots, Ukrainian test data for hyphenation and spell checking, and strings the engine matches against document content.
- CI runs `scripts/check-language.mjs`. It fails on Cyrillic in code comments and anywhere in Markdown. Strings in code are not checked.
- Intentionally kept non-English text goes in `.language-check.json` under `allow`, each entry with `path` (a glob, a file or a directory), `category` and `reason`. An entry that matches no file prints a warning.
- `scripts/check-language.mjs` is a vendored copy, kept in sync by hand. Do not change it here; changes go upstream first.

## Docs

`docs/ARCHITECTURE.md` describes the layers and the pipeline; read it first. The `README.md` has the commands and the crate table. The docs are Ukrainian and are being translated.
