# @studying-skills/spellengine

The StudyFlow speller: words of a paragraph checked against Hunspell dictionaries through `spellbook`.
Rust compiled to WebAssembly with `wasm-pack build crates/spell-wasm --release --target web`.

```
npm config set @studying-skills:registry https://npm.pkg.github.com
npm install @studying-skills/spellengine
```

```js
import init, * as speller from "@studying-skills/spellengine";
await init(); // or init({ module_or_path: url }) to choose where the .wasm is fetched from
```

It carries the same version as `@studying-skills/docengine`; both come from one version tag.

Source and issues: https://github.com/Studying-Skills/studying-skills-docengine. Licence: MIT.
