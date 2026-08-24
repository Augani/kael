#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="${workspace_dir}/target/browser-suite-smoke"
log_dir="${workspace_dir}/target/browser-suite-smoke-logs"
if [[ -n "${KAEL_SUITE_SMOKE_PORT:-}" ]]; then
  port="${KAEL_SUITE_SMOKE_PORT}"
else
  port="$(python3 - <<'PY'
import socket

with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
    listener.bind(("127.0.0.1", 0))
    print(listener.getsockname()[1])
PY
)"
fi

bash "${workspace_dir}/scripts/build-browser-suite-smoke.sh"
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
  if [[ "${browser_profile}" == /tmp/kael-suite-smoke.* ]]; then
    rm -rf "${browser_profile}"
  fi
}
trap cleanup EXIT

# Let an explicitly requested port fail here instead of accidentally treating
# an unrelated process already listening on that port as this smoke server.
sleep 0.1
if ! kill -0 "${server_pid}" 2>/dev/null; then
  echo "browser suite smoke server exited during startup" >&2
  cat "${log_dir}/http.log" >&2
  exit 1
fi

for attempt in {1..30}; do
  if ! kill -0 "${server_pid}" 2>/dev/null; then
    echo "browser suite smoke server exited before becoming ready" >&2
    cat "${log_dir}/http.log" >&2
    exit 1
  fi
  if curl --fail --silent "http://127.0.0.1:${port}/" > /dev/null; then
    break
  fi
  if [[ "${attempt}" == 30 ]]; then
    echo "browser suite smoke server did not become ready" >&2
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
  echo "Chrome or Chromium is required for the browser suite smoke" >&2
  exit 1
fi

browser_profile="$(mktemp -d /tmp/kael-suite-smoke.XXXXXX)"
browser_command=(
  "${browser}"
  --headless=new
  --no-sandbox
  --disable-background-networking
  --disable-component-update
  --disable-default-apps
  --disable-extensions
  --disable-sync
  --enable-webgl
  --enable-unsafe-swiftshader
  --use-angle=swiftshader
  --window-size=1280,720
  --remote-debugging-port=0
  --user-data-dir="${browser_profile}"
  "http://127.0.0.1:${port}/"
)
"${browser_command[@]}" > /dev/null 2> "${log_dir}/chrome.log" &
browser_pid=$!

pass_marker="/?__kael_suite_pass__=1&rows=1000000&columns=16384&blocks=250000&slides=10000&shapes=100000&selection=anchor_focus&pointer=passed&windows=passed&routes=passed&mounts=bounded&export=png"
for attempt in {1..60}; do
  if grep -Fq "${pass_marker}" "${log_dir}/http.log"; then
    break
  fi
  if grep -Fq "/?__kael_suite_failed__=1&" "${log_dir}/http.log"; then
    echo "browser suite smoke reported failure" >&2
    grep -F "/?__kael_suite_failed__=1&" "${log_dir}/http.log" >&2
    exit 1
  fi
  if ! kill -0 "${browser_pid}" 2>/dev/null; then
    echo "headless browser exited before the suite smoke completed" >&2
    exit 1
  fi
  if [[ "${attempt}" == 60 ]]; then
    echo "browser suite smoke timed out" >&2
    exit 1
  fi
  sleep 1
done

grep -Fq "${pass_marker}" "${log_dir}/http.log"

kill "${browser_pid}" 2>/dev/null || true
wait "${browser_pid}" 2>/dev/null || true
browser_pid=""
compact_browser_command=("${browser_command[@]}")
for index in "${!compact_browser_command[@]}"; do
  if [[ "${compact_browser_command[index]}" == --window-size=* ]]; then
    compact_browser_command[index]="--window-size=760,720"
  fi
done
"${compact_browser_command[@]}" > /dev/null 2>> "${log_dir}/chrome.log" &
browser_pid=$!

compact_marker="/?__kael_suite_compact_pass__=1&layout=compact&mounts=bounded"
for attempt in {1..60}; do
  if grep -Fq "${compact_marker}" "${log_dir}/http.log"; then
    break
  fi
  if grep -Fq "/?__kael_suite_failed__=1&" "${log_dir}/http.log"; then
    echo "compact browser suite smoke reported failure" >&2
    grep -F "/?__kael_suite_failed__=1&" "${log_dir}/http.log" >&2
    exit 1
  fi
  if ! kill -0 "${browser_pid}" 2>/dev/null; then
    echo "headless compact browser exited before the suite smoke completed" >&2
    exit 1
  fi
  if [[ "${attempt}" == 60 ]]; then
    echo "compact browser suite smoke timed out" >&2
    exit 1
  fi
  sleep 1
done

grep -Fq "${compact_marker}" "${log_dir}/http.log"
echo "Browser suite-scale wide and compact smokes passed; logs: ${log_dir}"
