#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="${workspace_dir}/target/browser-game-input-smoke"
log_dir="${workspace_dir}/target/browser-game-input-smoke-logs"
http_port="${KAEL_GAME_INPUT_HTTP_PORT:-8155}"

bash "${workspace_dir}/scripts/build-browser-game-input-smoke.sh"
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
  if [[ "${browser_profile}" == /tmp/kael-game-input-smoke.* ]]; then
    # Chromium can briefly recreate profile files while its child processes
    # finish exiting. Cleanup is best-effort and must not turn a passed runtime
    # proof into a false CI failure.
    for _cleanup_attempt in {1..3}; do
      rm -rf -- "${browser_profile}" 2>/dev/null || true
      [[ ! -e "${browser_profile}" ]] && break
      sleep 0.1
    done
  fi
}
trap cleanup EXIT

for attempt in {1..30}; do
  if curl --fail --silent "http://127.0.0.1:${http_port}/" > /dev/null; then
    break
  fi
  if ! kill -0 "${http_pid}" 2>/dev/null || [[ "${attempt}" == 30 ]]; then
    echo "browser game-input smoke HTTP server did not become ready" >&2
    cat "${log_dir}/http.log" >&2
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
  echo "Chrome or Chromium is required for the browser game-input smoke" >&2
  exit 1
fi

browser_profile="$(mktemp -d /tmp/kael-game-input-smoke.XXXXXX)"
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

pass_marker="/?__kael_game_input_pass__=1&capabilities=passed&gamepad=passed&lock=passed&movement=passed&unlock=passed&rejection=passed&synchronous=passed"
for attempt in {1..45}; do
  if grep -Fq "${pass_marker}" "${log_dir}/http.log"; then
    break
  fi
  if grep -Fq "/?__kael_game_input_failed__=1" "${log_dir}/http.log"; then
    echo "browser game-input smoke reported failure" >&2
    cat "${log_dir}/http.log" >&2
    cat "${log_dir}/chrome.log" >&2
    exit 1
  fi
  if ! kill -0 "${browser_pid}" 2>/dev/null || [[ "${attempt}" == 45 ]]; then
    echo "browser game-input smoke did not complete" >&2
    cat "${log_dir}/http.log" >&2
    cat "${log_dir}/chrome.log" >&2
    exit 1
  fi
  sleep 1
done

grep -Fq "${pass_marker}" "${log_dir}/http.log"
echo "Browser pointer-lock/gamepad/display-frame smoke passed; logs: ${log_dir}"
