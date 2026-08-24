#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
artifact_dir="${KAEL_BROWSER_MATRIX_ARTIFACT_DIR:-${workspace_dir}/target/browser-matrix}"
http_log="${artifact_dir}/http.log"
port="${KAEL_BROWSER_MATRIX_PORT:-8147}"
python_bin="${KAEL_PLAYWRIGHT_PYTHON:-python3}"
skip_build="${KAEL_BROWSER_MATRIX_SKIP_BUILD:-0}"
skip_suite="${KAEL_BROWSER_MATRIX_SKIP_SUITE:-0}"
skip_realtime="${KAEL_BROWSER_MATRIX_SKIP_REALTIME:-0}"
skip_capture="${KAEL_BROWSER_MATRIX_SKIP_CAPTURE:-0}"
engines="${KAEL_BROWSER_MATRIX_ENGINES:-chromium,firefox,webkit}"

if ! "${python_bin}" -c 'import playwright' >/dev/null 2>&1; then
  echo "Playwright 1.62.0 is required. Install it with:" >&2
  echo "  ${python_bin} -m pip install -r scripts/browser-matrix-requirements.txt" >&2
  echo "  ${python_bin} -m playwright install chromium firefox webkit" >&2
  exit 2
fi

installed_version="$("${python_bin}" -c 'import importlib.metadata; print(importlib.metadata.version("playwright"))')"
if [[ "${installed_version}" != "1.62.0" ]]; then
  echo "Playwright 1.62.0 is required (found ${installed_version})" >&2
  exit 2
fi

mkdir -p "${artifact_dir}"

if [[ "${skip_build}" != "1" ]]; then
  bash "${workspace_dir}/scripts/build-browser-smoke.sh"
  if [[ "${skip_capture}" != "1" ]]; then
    bash "${workspace_dir}/scripts/build-browser-capture-smoke.sh"
  fi
  if [[ "${skip_suite}" != "1" ]]; then
    bash "${workspace_dir}/scripts/build-browser-suite-smoke.sh"
  fi
  if [[ "${skip_realtime}" != "1" ]]; then
    bash "${workspace_dir}/scripts/build-browser-websocket-smoke.sh"
    cargo build --manifest-path "${workspace_dir}/Cargo.toml" \
      -p kael_net --example websocket_echo_server
  fi
fi

required_artifacts=(
  "${workspace_dir}/target/browser-smoke/index.html"
  "${workspace_dir}/target/browser-smoke/browser_smoke_bg.wasm"
)
if [[ "${skip_capture}" != "1" ]]; then
  required_artifacts+=(
    "${workspace_dir}/target/browser-capture-smoke/index.html"
    "${workspace_dir}/target/browser-capture-smoke/browser_capture_smoke_bg.wasm"
  )
fi
if [[ "${skip_suite}" != "1" ]]; then
  required_artifacts+=(
    "${workspace_dir}/target/browser-suite-smoke/index.html"
    "${workspace_dir}/target/browser-suite-smoke/suite_scale_smoke_bg.wasm"
  )
fi
if [[ "${skip_realtime}" != "1" ]]; then
  required_artifacts+=(
    "${workspace_dir}/target/browser-websocket-smoke/index.html"
    "${workspace_dir}/target/browser-websocket-smoke/browser_websocket_smoke_bg.wasm"
    "${workspace_dir}/target/debug/examples/websocket_echo_server"
  )
fi
for artifact in "${required_artifacts[@]}"; do
  if [[ ! -s "${artifact}" ]]; then
    echo "missing browser matrix artifact: ${artifact}" >&2
    exit 1
  fi
done

xvfb_pid=""
http_pid=""
cleanup() {
  if [[ -n "${http_pid}" ]]; then
    kill "${http_pid}" 2>/dev/null || true
    wait "${http_pid}" 2>/dev/null || true
  fi
  if [[ -n "${xvfb_pid}" ]]; then
    kill "${xvfb_pid}" 2>/dev/null || true
    wait "${xvfb_pid}" 2>/dev/null || true
  fi
}
trap cleanup EXIT

if [[ "$(uname -s)" == "Linux" && ",${engines}," == *,firefox,* ]]; then
  export KAEL_BROWSER_MATRIX_FIREFOX_HEADED=1
  export LIBGL_ALWAYS_SOFTWARE=1
  if [[ -z "${DISPLAY:-}" ]]; then
    if ! command -v Xvfb >/dev/null; then
      echo "Xvfb is required for Firefox's WebGL2 release proof on Linux" >&2
      echo "Install browser dependencies with: ${python_bin} -m playwright install-deps firefox" >&2
      exit 2
    fi
    xvfb_display="${KAEL_BROWSER_MATRIX_XVFB_DISPLAY:-97}"
    export DISPLAY=":${xvfb_display}"
    Xvfb "${DISPLAY}" -screen 0 1280x800x24 -nolisten tcp \
      > "${artifact_dir}/xvfb.log" 2>&1 &
    xvfb_pid=$!
    for attempt in {1..30}; do
      if [[ -S "/tmp/.X11-unix/X${xvfb_display}" ]]; then
        break
      fi
      if ! kill -0 "${xvfb_pid}" 2>/dev/null; then
        echo "browser matrix Xvfb exited before becoming ready" >&2
        cat "${artifact_dir}/xvfb.log" >&2
        exit 1
      fi
      if [[ "${attempt}" == 30 ]]; then
        echo "browser matrix Xvfb did not become ready" >&2
        exit 1
      fi
      sleep 1
    done
  fi
fi

"${python_bin}" -u -m http.server "${port}" --bind 127.0.0.1 --directory "${workspace_dir}/target" \
  > "${http_log}" 2>&1 &
http_pid=$!

for attempt in {1..30}; do
  if curl --fail --silent "http://127.0.0.1:${port}/browser-smoke/" >/dev/null; then
    break
  fi
  if ! kill -0 "${http_pid}" 2>/dev/null; then
    echo "browser matrix HTTP server exited before becoming ready" >&2
    cat "${http_log}" >&2
    exit 1
  fi
  if [[ "${attempt}" == 30 ]]; then
    echo "browser matrix HTTP server did not become ready" >&2
    exit 1
  fi
  sleep 1
done

runner_args=(
  --workspace "${workspace_dir}"
  --base-url "http://127.0.0.1:${port}"
  --artifacts "${artifact_dir}"
)
if [[ "${skip_suite}" == "1" ]]; then
  runner_args+=(--skip-suite)
fi
if [[ "${skip_capture}" == "1" ]]; then
  runner_args+=(--skip-capture)
fi
if [[ "${skip_realtime}" == "1" ]]; then
  runner_args+=(--skip-realtime)
else
  runner_args+=(--echo-server "${workspace_dir}/target/debug/examples/websocket_echo_server")
fi
runner_args+=(--engines "${engines}")

"${python_bin}" "${workspace_dir}/scripts/verify_browser_matrix.py" "${runner_args[@]}"
