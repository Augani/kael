#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="${workspace_dir}/target/browser-capture-smoke"
log_dir="${workspace_dir}/target/browser-capture-smoke-logs"
http_port="${KAEL_CAPTURE_HTTP_PORT:-8154}"

bash "${workspace_dir}/scripts/build-browser-capture-smoke.sh"
mkdir -p "${log_dir}"

python3 -u -m http.server "${http_port}" --bind 127.0.0.1 --directory "${output_dir}" \
  > "${log_dir}/http.log" 2>&1 &
http_pid=$!
browser_profile=""
browser_pid=""
cleanup() {
  if [[ -n "${browser_pid}" ]]; then
    kill "${browser_pid}" 2>/dev/null || true
    wait "${browser_pid}" 2>/dev/null || true
  fi
  kill "${http_pid}" 2>/dev/null || true
  wait "${http_pid}" 2>/dev/null || true
  if [[ "${browser_profile}" == /tmp/kael-capture-smoke.* ]]; then
    rm -rf "${browser_profile}"
  fi
}
trap cleanup EXIT

for attempt in {1..30}; do
  if curl --fail --silent "http://127.0.0.1:${http_port}/" > /dev/null; then
    break
  fi
  if ! kill -0 "${http_pid}" 2>/dev/null; then
    echo "browser capture smoke HTTP server exited before becoming ready" >&2
    cat "${log_dir}/http.log" >&2
    exit 1
  fi
  if [[ "${attempt}" == 30 ]]; then
    echo "browser capture smoke HTTP server did not become ready" >&2
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
  echo "Chrome or Chromium is required for the browser capture smoke" >&2
  exit 1
fi

browser_profile="$(mktemp -d /tmp/kael-capture-smoke.XXXXXX)"
"${browser}" \
  --headless=new \
  --no-sandbox \
  --disable-background-networking \
  --disable-component-update \
  --disable-default-apps \
  --disable-extensions \
  --disable-sync \
  --autoplay-policy=no-user-gesture-required \
  --remote-debugging-port=0 \
  --user-data-dir="${browser_profile}" \
  "http://127.0.0.1:${http_port}/" \
  > /dev/null 2> "${log_dir}/chrome.log" &
browser_pid=$!

pass_marker="/?__kael_capture_pass__=1&enumeration=passed&start=passed&frames=passed&lifecycle=passed&bounds=passed&async_error=passed"
for attempt in {1..45}; do
  if grep -Fq "${pass_marker}" "${log_dir}/http.log"; then
    break
  fi
  if grep -Fq "/?__kael_capture_failed__=1" "${log_dir}/http.log"; then
    echo "browser capture smoke reported failure" >&2
    cat "${log_dir}/http.log" >&2
    cat "${log_dir}/chrome.log" >&2
    exit 1
  fi
  if ! kill -0 "${browser_pid}" 2>/dev/null; then
    echo "headless browser exited before the capture smoke completed" >&2
    cat "${log_dir}/http.log" >&2
    cat "${log_dir}/chrome.log" >&2
    exit 1
  fi
  if [[ "${attempt}" == 45 ]]; then
    echo "browser capture smoke timed out" >&2
    cat "${log_dir}/http.log" >&2
    cat "${log_dir}/chrome.log" >&2
    exit 1
  fi
  sleep 1
done

grep -Fq "${pass_marker}" "${log_dir}/http.log"
echo "Browser display-capture lifecycle/frame/error smoke passed; logs: ${log_dir}"
