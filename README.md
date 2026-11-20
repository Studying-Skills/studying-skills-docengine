# studying-skills-docengine

Рушій верстки редактора звіту StudyFlow: документ → рядки → сторінки → PDF. Rust, збирається
нативно й у WebAssembly.

| Шар | Крейт | Завдання |
|---|---|---|
| 0 | `sfdoc-geom` | одиниці й прямокутники |
| 0 | `sfdoc-spell` | правопис: слова абзацу, яких немає в словнику, і варіанти виправлення |
| 1 | `sfdoc-model` | формат документа, позиції, заміна блоків |
| 1 | `sfdoc-fonts` | шрифти: метрики, формування гліфів, запасні шрифти |
| 2 | `sfdoc-layout` | верстка й список відображення |
| 3 | `sfdoc-raster` | PNG сторінки (тести й утиліта) |
| 3 | `sfdoc-pdf` | PDF зі списку відображення |
| 4 | `sfdoc-wasm` | прив'язки рушія верстки до JavaScript |
| 4 | `sfdoc-spell-wasm` | прив'язки правопису до JavaScript (окремий модуль wasm) |
| 4 | `sfdoc-cli` | утиліта `sfdoc` |

Нижчий шар не залежить від вищого: `cargo xtask layers`.

```
cargo test --workspace          # L1–L3
cargo xtask layers              # шари
cargo xtask ci                  # fmt, clippy, тести, шари, збирання обох модулів wasm, розмір wasm
cargo xtask wasm-size           # розмір уже зібраного wasm рушія проти бюджету
cargo run -p sfdoc-cli -- inspect corpus/plain/doc.json
cargo run -p sfdoc-cli -- render corpus/plain/doc.json -o page.png
```

Пакети: `@studying-skills/docengine` (рушій верстки) і `@studying-skills/spellengine` (правопис). Обидва
публікуються разом, з однією версією, з тегу `v*` (`.github/workflows/publish.yml`).

Архітектура: `docs/ARCHITECTURE.md`. Викидні прототипи: `spikes/` (буде видалено, #5).
