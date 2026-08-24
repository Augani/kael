#!/usr/bin/env bash
set -euo pipefail

run() {
  echo "+ $*"
  "$@"
}

usage() {
  cat <<'EOF' >&2
Usage: bash scripts/ci/verify-kael.sh [default|linux-x11|linux-wayland|macos-blade]
EOF
}

mode="${1:-default}"

case "$mode" in
  default)
    # Lint and test every crate, target, optional battery, template, and the
    # repository-only Astryx showcase. This is the workspace-wide quality gate.
    run cargo clippy --workspace --all-targets --all-features -- -D warnings
    run cargo test --workspace --all-targets --all-features
    run cargo check -p kael_http_client --no-default-features
    run cargo check -p kael --lib --features "platform-foundation"
    run cargo check -p kael --lib --features "document"
    run cargo check -p kael --lib --features "pdf"
    run cargo check -p kael --lib --features "office"
    run cargo check -p kael --lib --features "notifications-full"
    run cargo check -p kael --lib --features "share"
    run cargo clippy -p kael_notifications -p kael_share --all-targets -- -D warnings
    run cargo check -p kael --lib --features "platform-foundation document pdf office notifications-full share"
    run cargo check -p kael --bench framework
    run cargo run -p xtask -- dry-run
    ;;
  linux-x11)
    run cargo check -p kael --lib --no-default-features --features "font-kit x11"
    run cargo check -p kael --lib --no-default-features --features "font-kit x11 platform-foundation"
    run cargo check -p kael --lib --no-default-features --features "font-kit x11 platform-foundation document pdf office notifications-full share"
    run cargo check -p kael --bench framework --no-default-features --features "font-kit x11"
    run cargo clippy -p kael --lib --no-default-features --features "webview-legacy-gtk3" -- -D warnings
    ;;
  linux-wayland)
    run cargo check -p kael --lib --no-default-features --features "font-kit wayland"
    run cargo check -p kael --lib --no-default-features --features "font-kit wayland platform-foundation"
    run cargo check -p kael --lib --no-default-features --features "font-kit wayland platform-foundation document pdf office notifications-full share"
    run cargo check -p kael --bench framework --no-default-features --features "font-kit wayland"
    ;;
  macos-blade)
    run cargo check -p kael --lib --no-default-features --features "font-kit macos-blade"
    run cargo check -p kael --lib --no-default-features --features "font-kit macos-blade platform-foundation"
    run cargo check -p kael --lib --no-default-features --features "font-kit macos-blade platform-foundation document pdf office notifications-full share"
    ;;
  *)
    usage
    exit 1
    ;;
esac
