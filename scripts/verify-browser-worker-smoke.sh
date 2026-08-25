#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="${workspace_dir}/target/browser-worker-smoke"
log_dir="${workspace_dir}/target/browser-worker-smoke-logs"
port="${KAEL_WORKER_SMOKE_PORT:-8131}"

bash "${workspace_dir}/scripts/build-browser-worker-smoke.sh"
mkdir -p "${log_dir}"

python3 -u -m http.server "${port}" --bind 127.0.0.1 --directory "${output_dir}" \
  > "${log_dir}/http.log" 2>&1 &
server_pid=$!
browser_pid=""
browser_profile=""
cleanup() {
  if [[ -n "${browser_pid}" ]]; then
    kill "${browser_pid}" 2>/dev/null || true
    wait "${browser_pid}" 2>/dev/null || true
  fi
  kill "${server_pid}" 2>/dev/null || true
  wait "${server_pid}" 2>/dev/null || true
  if [[ "${browser_profile}" == /tmp/kael-worker-smoke.* ]]; then
    # Chromium child processes can briefly recreate profile files after the
    # launcher exits. Temporary cleanup must not override a passed proof.
    rm -rf -- "${browser_profile}" 2>/dev/null || true
  fi
}
trap cleanup EXIT

for attempt in {1..30}; do
  if curl --fail --silent "http://127.0.0.1:${port}/" > /dev/null; then
    break
  fi
  if [[ "${attempt}" == 30 ]]; then
    echo "browser worker smoke server did not become ready" >&2
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
  echo "Chrome or Chromium is required for the browser worker smoke" >&2
  exit 1
fi

browser_profile="$(mktemp -d /tmp/kael-worker-smoke.XXXXXX)"
browser_command=(
  "${browser}"
  --headless=new
  --no-sandbox
  --disable-background-networking
  --disable-component-update
  --disable-default-apps
  --disable-extensions
  --disable-sync
  --remote-debugging-port=0
  --user-data-dir="${browser_profile}"
  "http://127.0.0.1:${port}/"
)
"${browser_command[@]}" > /dev/null 2> "${log_dir}/chrome.log" &
browser_pid=$!

pass_marker="/?__kael_worker_pass__=1&protocol=1&items=1000000&progress=passed&ui_thread=responsive&terminated=passed"
for attempt in {1..30}; do
  if grep -Fq "${pass_marker}" "${log_dir}/http.log"; then
    break
  fi
  if grep -Fq "/?__kael_worker_failed__=1&" "${log_dir}/http.log"; then
    echo "browser worker smoke reported failure" >&2
    grep -F "/?__kael_worker_failed__=1&" "${log_dir}/http.log" >&2
    exit 1
  fi
  if ! kill -0 "${browser_pid}" 2>/dev/null; then
    echo "headless browser exited before the worker smoke completed" >&2
    exit 1
  fi
  if [[ "${attempt}" == 30 ]]; then
    echo "browser worker smoke timed out" >&2
    exit 1
  fi
  sleep 1
done

grep -Fq "${pass_marker}" "${log_dir}/http.log"

echo "Browser worker smoke passed; logs: ${log_dir}"
