#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="${workspace_dir}/target/browser-websocket-smoke"
log_dir="${workspace_dir}/target/browser-websocket-smoke-logs"
http_port="${KAEL_WEBSOCKET_HTTP_PORT:-8143}"
websocket_port="${KAEL_WEBSOCKET_PORT:-8134}"

if [[ "${websocket_port}" != "8134" ]]; then
  echo "browser WebSocket smoke currently requires KAEL_WEBSOCKET_PORT=8134" >&2
  exit 2
fi

bash "${workspace_dir}/scripts/build-browser-websocket-smoke.sh"
mkdir -p "${log_dir}"

cargo run --manifest-path "${workspace_dir}/Cargo.toml" \
  -p kael_net --example websocket_echo_server -- "${websocket_port}" \
  > "${log_dir}/websocket.log" 2>&1 &
websocket_pid=$!
python3 -u -m http.server "${http_port}" --bind 127.0.0.1 --directory "${output_dir}" \
  > "${log_dir}/http.log" 2>&1 &
http_pid=$!
browser_pid=""
browser_profile=""
cleanup() {
  if [[ -n "${browser_pid}" ]]; then
    kill "${browser_pid}" 2>/dev/null || true
    wait "${browser_pid}" 2>/dev/null || true
  fi
  kill "${http_pid}" 2>/dev/null || true
  wait "${http_pid}" 2>/dev/null || true
  kill "${websocket_pid}" 2>/dev/null || true
  wait "${websocket_pid}" 2>/dev/null || true
  if [[ "${browser_profile}" == /tmp/kael-websocket-smoke.* ]]; then
    rm -rf "${browser_profile}"
  fi
}
trap cleanup EXIT

for attempt in {1..60}; do
  if grep -Fq "KAEL_WEBSOCKET_ECHO_READY" "${log_dir}/websocket.log" && \
      curl --fail --silent "http://127.0.0.1:${http_port}/" > /dev/null; then
    break
  fi
  if ! kill -0 "${websocket_pid}" 2>/dev/null; then
    echo "local WebSocket echo server exited before becoming ready" >&2
    cat "${log_dir}/websocket.log" >&2
    exit 1
  fi
  if ! kill -0 "${http_pid}" 2>/dev/null; then
    echo "local WebSocket smoke HTTP server exited before becoming ready" >&2
    cat "${log_dir}/http.log" >&2
    exit 1
  fi
  if [[ "${attempt}" == 60 ]]; then
    echo "browser WebSocket smoke servers did not become ready" >&2
    exit 1
  fi
  sleep 1
done

browser="${KAEL_BROWSER_BIN:-}"
if [[ -z "${browser}" ]]; then
  browser="$(command -v google-chrome || command -v google-chrome-stable || command -v chromium || true)"
fi
if [[ -z "${browser}" && -x "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" ]]; then
  browser="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
fi
if [[ -z "${browser}" ]]; then
  echo "Chrome or Chromium is required for the browser WebSocket smoke" >&2
  exit 1
fi

browser_profile="$(mktemp -d /tmp/kael-websocket-smoke.XXXXXX)"
"${browser}" \
  --headless=new \
  --no-sandbox \
  --disable-background-networking \
  --disable-component-update \
  --disable-default-apps \
  --disable-extensions \
  --disable-sync \
  --remote-debugging-port=0 \
  --user-data-dir="${browser_profile}" \
  "http://127.0.0.1:${http_port}/" \
  > /dev/null 2> "${log_dir}/chrome.log" &
browser_pid=$!

pass_marker="/?__kael_websocket_pass__=1&protocol=passed&text=passed&binary=passed&ordered=passed&close=passed&error=passed&cancellation=passed&backpressure=passed&policy=passed&size=passed&reconnect=passed"
for attempt in {1..45}; do
  if grep -Fq "${pass_marker}" "${log_dir}/http.log"; then
    break
  fi
  if grep -Fq "/?__kael_websocket_failed__=1&" "${log_dir}/http.log"; then
    echo "browser WebSocket smoke reported failure" >&2
    grep -F "/?__kael_websocket_failed__=1&" "${log_dir}/http.log" >&2
    exit 1
  fi
  if ! kill -0 "${browser_pid}" 2>/dev/null; then
    echo "headless browser exited before the WebSocket smoke completed" >&2
    exit 1
  fi
  if [[ "${attempt}" == 45 ]]; then
    echo "browser WebSocket smoke timed out" >&2
    cat "${log_dir}/http.log" >&2
    cat "${log_dir}/websocket.log" >&2
    exit 1
  fi
  sleep 1
done

grep -Fq "${pass_marker}" "${log_dir}/http.log"
echo "Browser WebSocket smoke passed; logs: ${log_dir}"
