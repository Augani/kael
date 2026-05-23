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
    run cargo clippy -p kael --lib -- -D warnings
    run cargo test -p kael --lib -- --test-threads=1
    run cargo test -p kael --test worker_process
    run cargo test -p kael --test extension_process
    run cargo check -p kael --lib --features "platform-foundation"
    run cargo check -p kael --lib --features "document"
    run cargo check -p kael --lib --features "audio"
    run cargo check -p kael --lib --features "pdf"
    run cargo check -p kael --lib --features "notifications-full"
    run cargo check -p kael --lib --features "share"
    run cargo check -p kael --lib --features "platform-foundation document audio pdf notifications-full share"
    run cargo check -p kael --example platform_features --example daemon_app --example perf_bench --example capture_demo
    run cargo run -p xtask -- dry-run
    ;;
  linux-x11)
    run cargo check -p kael --lib --no-default-features --features "font-kit x11"
    run cargo check -p kael --lib --no-default-features --features "font-kit x11 platform-foundation"
    run cargo check -p kael --lib --no-default-features --features "font-kit x11 platform-foundation document audio pdf notifications-full share"
    run cargo check -p kael --example platform_features --example daemon_app --example perf_bench --example capture_demo --no-default-features --features "font-kit x11"
    ;;
  linux-wayland)
    run cargo check -p kael --lib --no-default-features --features "font-kit wayland"
    run cargo check -p kael --lib --no-default-features --features "font-kit wayland platform-foundation"
    run cargo check -p kael --lib --no-default-features --features "font-kit wayland platform-foundation document audio pdf notifications-full share"
    run cargo check -p kael --example platform_features --example daemon_app --example perf_bench --example capture_demo --no-default-features --features "font-kit wayland"
    ;;
  macos-blade)
    run cargo check -p kael --lib --no-default-features --features "font-kit macos-blade"
    run cargo check -p kael --lib --no-default-features --features "font-kit macos-blade platform-foundation"
    run cargo check -p kael --lib --no-default-features --features "font-kit macos-blade platform-foundation document audio pdf notifications-full share"
    ;;
  *)
    usage
    exit 1
    ;;
esac
