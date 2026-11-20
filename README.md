# studying-skills-docengine

Рушій верстки редактора звіту StudyFlow: документ → рядки → сторінки → PDF. Rust, збирається
нативно й у WebAssembly.

| Шар | Крейт | Завдання |
|---|---|---|
| 0 | `sfdoc-geom` | одиниці й прямокутники |
| 1 | `sfdoc-model` | формат документа, позиції, заміна блоків |
| 1 | `sfdoc-fonts` | шрифти: метрики, формування гліфів, запасні шрифти |
| 2 | `sfdoc-layout` | верстка й список відображення |
| 3 | `sfdoc-raster` | PNG сторінки (тести й утиліта) |
| 3 | `sfdoc-pdf` | PDF зі списку відображення |
| 4 | `sfdoc-wasm` | прив'язки до JavaScript |
| 4 | `sfdoc-cli` | утиліта `sfdoc` |

Нижчий шар не залежить від вищого: `cargo xtask layers`.

```
cargo test                      # L1–L3
cargo xtask layers              # шари
cargo xtask ci                  # fmt, clippy, тести, шари, збирання wasm, розмір wasm
cargo run -p sfdoc-cli -- inspect corpus/plain/doc.json
cargo run -p sfdoc-cli -- render corpus/plain/doc.json -o page.png
```

Архітектура: `docs/ARCHITECTURE.md`. Викидні прототипи: `spikes/` (буде видалено, #5).
