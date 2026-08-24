#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
log_dir="${workspace_dir}/target/macos-wkwebview-smoke"
log_file="${log_dir}/wkwebview.log"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "the WKWebView runtime smoke requires macOS" >&2
  exit 2
fi

mkdir -p "${log_dir}"
(
  unset KAEL_HEADLESS
  if [[ -n "${KAEL_HEADLESS+x}" ]]; then
    echo "KAEL_HEADLESS remained set after the runtime-smoke unset" >&2
    exit 1
  fi
  cargo run --manifest-path "${workspace_dir}/Cargo.toml" \
    -p kael --example webview_smoke \
    --no-default-features --features webview,runtime_shaders
) 2>&1 | tee "${log_file}"

if grep -Fq "WEBVIEW_SMOKE_FAIL:" "${log_file}"; then
  echo "WKWebView runtime smoke reported failure" >&2
  exit 1
fi
for marker in \
  "WEBVIEW_SMOKE_STAGE: custom-protocol" \
  "WEBVIEW_SMOKE_STAGE: page-load-finished" \
  "WEBVIEW_SMOKE_STAGE: page-to-host-ipc" \
  "WEBVIEW_SMOKE_STAGE: javascript-result" \
  "WEBVIEW_SMOKE_STAGE: current-url" \
  "WEBVIEW_SMOKE_STAGE: host-message-round-trip" \
  "WEBVIEW_SMOKE_OK:" \
  "Kael WebView smoke:42" \
  "|url=kael-smoke://assets/probe" \
  "|pong=42"; do
  if ! grep -Fq "${marker}" "${log_file}"; then
    echo "WKWebView runtime smoke did not publish: ${marker}" >&2
    exit 1
  fi
done

echo "macOS WKWebView runtime/IPC/JavaScript smoke passed; log: ${log_file}"
