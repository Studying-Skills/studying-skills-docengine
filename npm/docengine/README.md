# @studying-skills/docengine

The StudyFlow document layout engine: a document goes in; lines, pages, a display list and a PDF come out.
Rust compiled to WebAssembly with `wasm-pack build crates/wasm --release --target web`.

```
npm config set @studying-skills:registry https://npm.pkg.github.com
npm install @studying-skills/docengine
```

```js
import init, * as engine from "@studying-skills/docengine";
await init(); // or init({ module_or_path: url }) to choose where the .wasm is fetched from
```

Both engine packages are published together from one version tag and always carry the same version.
The speller is a separate package, `@studying-skills/spellengine`, so a page loads it only when spelling is on.

Source and issues: https://github.com/Studying-Skills/studying-skills-docengine. Licence: MIT.
