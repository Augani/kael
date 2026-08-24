#!/usr/bin/env bash
set -euo pipefail

# WebKitGTK uses the session bus for process and portal integration. Keep the
# proof self-contained on minimal CI runners while preserving an existing bus.
if [[ -z "${DBUS_SESSION_BUS_ADDRESS:-}" \
  && "${KAEL_WEBVIEW_GTK4_DBUS_REEXEC:-0}" != "1" ]] \
  && command -v dbus-run-session >/dev/null 2>&1; then
  exec dbus-run-session -- env KAEL_WEBVIEW_GTK4_DBUS_REEXEC=1 bash "$0" "$@"
fi

repo_root="${KAEL_REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
artifact_dir="${KAEL_WEBVIEW_GTK4_ARTIFACT_DIR:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/kael-webview-wayland-gtk4}"
runtime_dir="${KAEL_WEBVIEW_GTK4_RUNTIME_DIR:-${artifact_dir}/runtime}"
wayland_socket="${KAEL_WEBVIEW_GTK4_SOCKET:-kael-webview-gtk4-ci}"
weston_log="${artifact_dir}/kael-weston.log"
smoke_log="${artifact_dir}/kael-linux-webview-wayland-gtk4.log"
platform_smoke_log="${artifact_dir}/kael-linux-webview-wayland-gtk4-platform.log"
screenshot_path="${KAEL_WEBVIEW_GTK4_SCREENSHOT_PATH:-}"

mkdir -p "${artifact_dir}" "${runtime_dir}"
chmod 700 "${runtime_dir}"

if ! command -v weston >/dev/null 2>&1; then
  echo "WEBVIEW_WAYLAND_GTK4_FAIL: weston is not installed" >&2
  exit 1
fi

weston_pid=
smoke_pid=
screenshot_temp_dir=
cleanup() {
  if [[ -n "${smoke_pid}" ]]; then
    kill "${smoke_pid}" 2>/dev/null || true
    wait "${smoke_pid}" 2>/dev/null || true
  fi
  if [[ -n "${weston_pid}" ]]; then
    kill "${weston_pid}" 2>/dev/null || true
    wait "${weston_pid}" 2>/dev/null || true
  fi
  if [[ -n "${screenshot_temp_dir}" ]]; then
    rmdir "${screenshot_temp_dir}" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

export XDG_RUNTIME_DIR="${runtime_dir}"
unset DISPLAY
# CI has no physical render node. Mesa's software driver still exercises the
# compositor/client EGL and DMA-BUF path deterministically.
export LIBGL_ALWAYS_SOFTWARE=1

start_weston() {
  : > "${weston_log}"
  weston \
    --backend=headless-backend.so \
    --debug \
    --socket="${wayland_socket}" \
    --idle-time=0 \
    --width=1280 \
    --height=800 \
    "$@" \
    > "${weston_log}" 2>&1 &
  weston_pid=$!
}

start_weston --renderer=gl
for _ in {1..20}; do
  [[ -S "${runtime_dir}/${wayland_socket}" ]] && break
  if ! kill -0 "${weston_pid}" 2>/dev/null; then
    wait "${weston_pid}" 2>/dev/null || true
    weston_pid=
    break
  fi
  sleep 0.25
done

if [[ ! -S "${runtime_dir}/${wayland_socket}" ]]; then
  cleanup
  weston_pid=
  start_weston --renderer=pixman
fi

for _ in {1..80}; do
  [[ -S "${runtime_dir}/${wayland_socket}" ]] && break
  if ! kill -0 "${weston_pid}" 2>/dev/null; then
    echo "WEBVIEW_WAYLAND_GTK4_FAIL: Weston exited before publishing its socket" >&2
    cat "${weston_log}" >&2
    exit 1
  fi
  sleep 0.25
done
if [[ ! -S "${runtime_dir}/${wayland_socket}" ]]; then
  echo "WEBVIEW_WAYLAND_GTK4_FAIL: timed out waiting for the Wayland socket" >&2
  cat "${weston_log}" >&2
  exit 1
fi

export WAYLAND_DISPLAY="${wayland_socket}"
export GDK_BACKEND=wayland
unset KAEL_HEADLESS KAEL_LINUX_BACKEND

cd "${repo_root}"
if [[ -n "${screenshot_path}" ]]; then
  mkdir -p "$(dirname "${screenshot_path}")"
  if [[ -e "${screenshot_path}" ]]; then
    echo "WEBVIEW_WAYLAND_GTK4_FAIL: screenshot target already exists: ${screenshot_path}" >&2
    exit 1
  fi
  export KAEL_WEBVIEW_WAYLAND_EVIDENCE_HOLD_MS="${KAEL_WEBVIEW_WAYLAND_EVIDENCE_HOLD_MS:-5000}"
  cargo run --locked -p kael --example webview_wayland_gtk4_smoke \
    --no-default-features \
    --features webview-wayland-gtk4 \
    > "${smoke_log}" 2>&1 &
  smoke_pid=$!

  # A clean release runner may need to compile the GTK/WebKit stack before the
  # example can publish readiness. Keep this bounded, but do not mistake a cold
  # build for a compositor failure.
  for _ in {1..1800}; do
    grep -q '^WEBVIEW_WAYLAND_GTK4_OK:' "${smoke_log}" 2>/dev/null && break
    if ! kill -0 "${smoke_pid}" 2>/dev/null; then
      wait "${smoke_pid}" || true
      smoke_pid=
      cat "${smoke_log}" >&2
      echo "WEBVIEW_WAYLAND_GTK4_FAIL: smoke exited before screenshot readiness" >&2
      exit 1
    fi
    sleep 0.1
  done
  if ! grep -q '^WEBVIEW_WAYLAND_GTK4_OK:' "${smoke_log}"; then
    cat "${smoke_log}" >&2
    echo "WEBVIEW_WAYLAND_GTK4_FAIL: timed out waiting for screenshot readiness" >&2
    exit 1
  fi
  screenshot_temp_dir="$(mktemp -d)"
  (
    cd "${screenshot_temp_dir}"
    weston-screenshooter
  )
  shopt -s nullglob
  captured_screenshots=("${screenshot_temp_dir}"/wayland-screenshot-*.png)
  shopt -u nullglob
  if [[ "${#captured_screenshots[@]}" -ne 1 ]]; then
    echo "WEBVIEW_WAYLAND_GTK4_FAIL: expected one Weston screenshot" >&2
    exit 1
  fi
  mv "${captured_screenshots[0]}" "${screenshot_path}"
  rmdir "${screenshot_temp_dir}"
  screenshot_temp_dir=
  wait "${smoke_pid}"
  smoke_pid=
  cat "${smoke_log}"
  echo "WEBVIEW_WAYLAND_GTK4_SCREENSHOT: ${screenshot_path}"
else
  cargo run --locked -p kael --example webview_wayland_gtk4_smoke \
    --no-default-features \
    --features webview-wayland-gtk4 \
    2>&1 | tee "${smoke_log}"
fi

grep -q '^WEBVIEW_WAYLAND_GTK4_BACKEND: GdkWaylandDisplay$' "${smoke_log}"
grep -q '^WEBVIEW_WAYLAND_GTK4_STAGE: kael-gsk-scene-realized$' "${smoke_log}"
grep -q '^WEBVIEW_WAYLAND_GTK4_STAGE: same-gdk-surface$' "${smoke_log}"
grep -q '^WEBVIEW_WAYLAND_GTK4_STAGE: visible-webview-allocation$' "${smoke_log}"
grep -Eq '^WEBVIEW_WAYLAND_GTK4_ALLOCATION: [4-9][0-9]{2,}x[3-9][0-9]{2,}$' "${smoke_log}"
grep -q '^WEBVIEW_WAYLAND_GTK4_STAGE: page-to-host-ipc$' "${smoke_log}"
grep -q '^WEBVIEW_WAYLAND_GTK4_STAGE: javascript-result$' "${smoke_log}"
grep -q '^WEBVIEW_WAYLAND_GTK4_OK:' "${smoke_log}"

# Exercise the actual Kael PlatformWindow implementation as well as the focused
# GTK composition proof above. This catches regressions in backend selection,
# retained-scene delivery, declarative WebView synchronization, command routing
# and GTK-owned application shutdown.
cargo run --locked -p kael --example webview_gtk4_platform_smoke \
  --no-default-features \
  --features webview-wayland-gtk4 \
  2>&1 | tee "${platform_smoke_log}"
grep -q '^WEBVIEW_SMOKE_BACKEND: Wayland$' "${platform_smoke_log}"
grep -q '^WEBVIEW_SMOKE_STAGE: retained-scene-png$' "${platform_smoke_log}"
grep -q '^WEBVIEW_SMOKE_STAGE: gsk-gpu-specs ' "${platform_smoke_log}"
grep -q '^WEBVIEW_SMOKE_STAGE: idle-event-driven$' "${platform_smoke_log}"
grep -q '^WEBVIEW_SMOKE_STAGE: page-to-host-ipc$' "${platform_smoke_log}"
grep -q '^WEBVIEW_SMOKE_STAGE: javascript-result$' "${platform_smoke_log}"
grep -q '^WEBVIEW_SMOKE_STAGE: raw-platform-handles-Wayland$' "${platform_smoke_log}"
grep -q '^WEBVIEW_SMOKE_STAGE: native-pointer-lock$' "${platform_smoke_log}"
grep -q '^WEBVIEW_SMOKE_STAGE: native-context-menu$' "${platform_smoke_log}"
grep -q '^WEBVIEW_SMOKE_STAGE: custom-protocol$' "${platform_smoke_log}"
grep -q '^WEBVIEW_SMOKE_OK: .*url=kael-smoke://assets/probe' "${platform_smoke_log}"
if grep -q 'RefCell already borrowed' "${platform_smoke_log}"; then
  echo "WEBVIEW_WAYLAND_GTK4_FAIL: synchronous GTK callback re-entered borrowed Kael state" >&2
  exit 1
fi
echo "WEBVIEW_WAYLAND_GTK4_PLATFORM_OK: production PlatformWindow host"

if [[ -n "${DISPLAY:-}" ]]; then
  echo "WEBVIEW_WAYLAND_GTK4_FAIL: DISPLAY unexpectedly became available" >&2
  exit 1
fi

echo "WEBVIEW_WAYLAND_GTK4_RUNTIME_OK: WAYLAND_DISPLAY=${wayland_socket} DISPLAY=unset"
