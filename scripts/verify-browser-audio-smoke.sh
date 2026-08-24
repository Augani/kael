#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="${workspace_dir}/target/browser-audio-smoke"
log_dir="${workspace_dir}/target/browser-audio-smoke-logs"
http_port="${KAEL_AUDIO_HTTP_PORT:-8153}"

bash "${workspace_dir}/scripts/build-browser-audio-smoke.sh"
mkdir -p "${log_dir}"

python3 -u -m http.server "${http_port}" --bind 127.0.0.1 --directory "${output_dir}" \
  > "${log_dir}/http.log" 2>&1 &
http_pid=$!
browser_profile=""
browser_pid=""
remove_browser_profile() {
  local profile="$1"
  if [[ "${profile}" != /tmp/kael-audio-smoke.* ]]; then
    return 0
  fi
  for _ in {1..10}; do
    rm -rf -- "${profile}" 2>/dev/null || true
    if [[ ! -e "${profile}" ]]; then
      return 0
    fi
    # Chrome can briefly keep writing profile databases after its launcher has
    # exited. Cleanup is best-effort and must not turn a passed audio probe red.
    sleep 0.2
  done
  echo "warning: browser audio smoke profile is still being released: ${profile}" >&2
  return 0
}
cleanup() {
  if [[ -n "${browser_pid}" ]]; then
    kill "${browser_pid}" 2>/dev/null || true
    wait "${browser_pid}" 2>/dev/null || true
  fi
  kill "${http_pid}" 2>/dev/null || true
  wait "${http_pid}" 2>/dev/null || true
  remove_browser_profile "${browser_profile}"
}
trap cleanup EXIT

for attempt in {1..30}; do
  if ! kill -0 "${http_pid}" 2>/dev/null; then
    echo "browser audio smoke HTTP server exited before becoming ready" >&2
    cat "${log_dir}/http.log" >&2
    exit 1
  fi
  if curl --fail --silent "http://127.0.0.1:${http_port}/" > /dev/null; then
    break
  fi
  if [[ "${attempt}" == 30 ]]; then
    echo "browser audio smoke HTTP server did not become ready" >&2
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
  echo "Chrome or Chromium is required for the browser audio smoke" >&2
  exit 1
fi

pass_marker="/?__kael_audio_pass__=1&worklet=passed&resume=passed&clock=passed&control=passed&bounds=passed&devices=passed&capture_denied=passed&cleanup=passed"
failure_marker="/?__kael_audio_failed__=1"
probe_passed=0
probe_failure="browser audio smoke did not complete"
for run_attempt in 1 2; do
  pass_count_before="$(grep -Fc "${pass_marker}" "${log_dir}/http.log" || true)"
  failure_count_before="$(grep -Fc "${failure_marker}" "${log_dir}/http.log" || true)"
  browser_profile="$(mktemp -d /tmp/kael-audio-smoke.XXXXXX)"
  "${browser}" \
    --headless=new \
    --no-sandbox \
    --disable-background-networking \
    --disable-component-update \
    --disable-default-apps \
    --disable-extensions \
    --disable-sync \
    --autoplay-policy=no-user-gesture-required \
    --deny-permission-prompts \
    --use-fake-device-for-media-stream \
    --remote-debugging-port=0 \
    --user-data-dir="${browser_profile}" \
    "http://127.0.0.1:${http_port}/" \
    > /dev/null 2>> "${log_dir}/chrome.log" &
  browser_pid=$!

  for attempt in {1..45}; do
    pass_count="$(grep -Fc "${pass_marker}" "${log_dir}/http.log" || true)"
    if (( pass_count > pass_count_before )); then
      probe_passed=1
      break
    fi
    failure_count="$(grep -Fc "${failure_marker}" "${log_dir}/http.log" || true)"
    if (( failure_count > failure_count_before )); then
      probe_failure="browser audio smoke reported failure"
      break
    fi
    if ! kill -0 "${browser_pid}" 2>/dev/null; then
      probe_failure="headless browser exited before the audio smoke completed"
      break
    fi
    if [[ "${attempt}" == 45 ]]; then
      probe_failure="browser audio smoke timed out"
      break
    fi
    sleep 1
  done

  if (( probe_passed == 1 )); then
    break
  fi
  kill "${browser_pid}" 2>/dev/null || true
  wait "${browser_pid}" 2>/dev/null || true
  browser_pid=""
  remove_browser_profile "${browser_profile}"
  browser_profile=""
  if [[ "${run_attempt}" == 1 ]]; then
    echo "${probe_failure}; retrying once after browser audio teardown" >&2
    sleep 2
  fi
done

if (( probe_passed != 1 )); then
  echo "${probe_failure}" >&2
  cat "${log_dir}/http.log" >&2
  cat "${log_dir}/chrome.log" >&2
  exit 1
fi

grep -Fq "${pass_marker}" "${log_dir}/http.log"

echo "Browser AudioWorklet/device/capture smoke passed; logs: ${log_dir}"
