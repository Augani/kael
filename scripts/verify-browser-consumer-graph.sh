#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${workspace_dir}"

cargo clippy -p kael --lib --target wasm32-unknown-unknown \
  --no-default-features --features browser-full -- -D warnings

cargo clippy -p kael_ui --lib --target wasm32-unknown-unknown \
  --no-default-features \
  --features browser,editor-languages,markdown,html-render,http,media,audio,image-avif,image-exr \
  -- -D warnings

echo "Maximum browser consumer graphs passed strict Clippy"
