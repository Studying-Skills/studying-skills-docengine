#!/bin/sh
# Assembles the two npm packages into dist/npm/<name> from the wasm-pack output of crates/wasm and crates/spell-wasm
# (build them first: `wasm-pack build <crate> --release --target web --no-pack`).
# Usage: scripts/pack-npm.sh [VERSION]   (default: the version in npm/<name>/package.json)
# Then `npm pack --dry-run` or `npm publish` inside each directory.
set -eu
cd "$(dirname "$0")/.."
OUT=dist/npm
VERSION="${1:-}"
rm -rf "$OUT"
assemble() { # name crate wasm-file-stem
  dir="$OUT/$1"
  mkdir -p "$dir"
  for f in "$3.js" "$3.d.ts" "$3_bg.wasm" "$3_bg.wasm.d.ts"; do
    [ -f "crates/$2/pkg/$f" ] || { echo "missing crates/$2/pkg/$f: build the wasm first" >&2; exit 1; }
    cp "crates/$2/pkg/$f" "$dir/"
  done
  cp "npm/$1/package.json" "npm/$1/README.md" LICENSE "$dir/"
  if [ -n "$VERSION" ]; then
    (cd "$dir" && npm version "$VERSION" --no-git-tag-version --allow-same-version >/dev/null)
  fi
}
assemble docengine wasm sfdoc_wasm
assemble spellengine spell-wasm sfdoc_spell_wasm
echo "assembled $OUT/docengine and $OUT/spellengine ${VERSION:+at version $VERSION}"
