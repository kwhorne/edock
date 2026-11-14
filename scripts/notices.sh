#!/bin/sh
# Regenerates desktop/NOTICE: the licences of everything built into the
# eDock app (its Rust crates, and the JavaScript libraries in its window).
# The engine has its own: engine/scripts/ci/notices.sh writes engine/NOTICE.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root/desktop"
tmp=$(mktemp "${TMPDIR:-/tmp}/edock-app-notice.XXXXXX")
trap 'rm -f "$tmp"' EXIT
command -v cargo-about >/dev/null 2>&1 || { echo "needs cargo-about: cargo install cargo-about --locked --features cli" >&2; exit 1; }
cargo about generate --locked about.hbs -o "$tmp"
printf '\nJavaScript libraries in the window\n==================================\n' >> "$tmp"
# The frontend's own dependencies, and Svelte, which the build compiles in.
for pkg in $(node -p 'Object.keys(require("./app/package.json").dependencies).join(" ")') svelte; do
  dir="app/node_modules/$pkg"
  meta=$(node -p "const p=require('./$dir/package.json'); p.name+' '+p.version+' — '+p.license")
  printf '\n%s\n----------------------------------------------------------------------\n\n' "$meta" >> "$tmp"
  lic=$(ls "$dir" | grep -iE '^licen[sc]e' | head -1 || true)
  if [ -n "$lic" ]; then
    cat "$dir/$lic" >> "$tmp"
  else
    # Elyra's packages ship without a licence file: it's the framework's MIT.
    printf 'Copyright (c) 2026 Knut W. Horne and the Elyra contributors.\nMIT License: https://github.com/kwhorne/elyra-framework/blob/main/LICENSE\n' >> "$tmp"
  fi
  printf '\n' >> "$tmp"
done
sed -e 's/\r$//' -e 's/[[:blank:]]*$//' "$tmp" > NOTICE
echo "wrote desktop/NOTICE ($(wc -l < NOTICE | tr -d ' ') lines)"
