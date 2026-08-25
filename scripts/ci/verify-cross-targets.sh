#!/usr/bin/env bash
set -euo pipefail

run() {
  echo "+ $*"
  "$@"
}

for command in cargo cargo-zigbuild zig; do
  if ! command -v "${command}" >/dev/null 2>&1; then
    echo "${command} is required for the local cross-target preflight" >&2
    exit 2
  fi
done

linux_target="${KAEL_ZIG_LINUX_TARGET:-x86_64-unknown-linux-gnu.2.31}"

# Zig supplies a reproducible Linux linker and glibc floor from macOS without
# pretending to execute Linux, WebView, or GPU code. Tests are built so the
# portable crates reach their final link step rather than stopping at metadata.
run cargo zigbuild --locked --target "${linux_target}" --tests \
  -p kael_cache \
  -p kael_collections \
  -p kael_document \
  -p kael_engines \
  -p kael_gpu_budget \
  -p kael_render_graph \
  -p kael_storage

# WebAssembly is a distinct target, not a Linux cross-build. Keep its public
# framework and UI graphs in the same pre-push command so a desktop change
# cannot silently break the one-codebase browser build.
run cargo check --locked -p kael --lib --target wasm32-unknown-unknown \
  --no-default-features \
  --features browser,storage,document,pdf,office,notifications-full,share,screen-capture
run cargo check --locked -p kael_ui --lib --target wasm32-unknown-unknown \
  --no-default-features \
  --features browser,editor-languages,markdown,html-render,http,media,audio,screen-capture
run cargo check --locked -p kael_engines --lib --target wasm32-unknown-unknown \
  --no-default-features

echo "Cross-target preflight passed for ${linux_target} and wasm32-unknown-unknown."
