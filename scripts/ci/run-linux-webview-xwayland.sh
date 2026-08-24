#!/usr/bin/env bash
set -euo pipefail

# WebKitGTK expects a session bus even in a headless compositor. GitHub-hosted
# Linux runners do not consistently publish one, so make the smoke self-contained
# without nesting when a caller (or a local desktop) already supplied a bus.
if [[ -z "${DBUS_SESSION_BUS_ADDRESS:-}" \
  && "${KAEL_WEBVIEW_DBUS_REEXEC:-0}" != "1" ]] \
  && command -v dbus-run-session >/dev/null 2>&1; then
  exec dbus-run-session -- env KAEL_WEBVIEW_DBUS_REEXEC=1 bash "$0" "$@"
fi

# Prove the Linux WebView contract on a real Wayland desktop path. Weston owns
# the desktop session and its built-in XWM owns XWayland; Kael must then select
# the maintained GTK4/GSK + WebKitGTK 6 X11 host while WAYLAND_DISPLAY is also
# valid. This is intentionally stronger than setting a fake Wayland variable
# beside Xvfb.

repo_root="${KAEL_REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
artifact_dir="${KAEL_WEBVIEW_ARTIFACT_DIR:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/kael-webview-xwayland}"
runtime_dir="${KAEL_WEBVIEW_RUNTIME_DIR:-${artifact_dir}/runtime}"
wayland_socket="${KAEL_WEBVIEW_WAYLAND_SOCKET:-kael-webview-ci}"
weston_log="${artifact_dir}/kael-weston.log"
smoke_log="${artifact_dir}/kael-linux-webview.log"

mkdir -p "${artifact_dir}" "${runtime_dir}"
chmod 700 "${runtime_dir}"
# Minimal/headless images do not always create this conventional X socket
# directory. Xwayland requires it even though Weston chooses the display number.
mkdir -p /tmp/.X11-unix
chmod 1777 /tmp/.X11-unix

if ! command -v weston >/dev/null 2>&1; then
  echo "WESTON_XWAYLAND_FAIL: weston is not installed" >&2
  exit 1
fi
if ! command -v Xwayland >/dev/null 2>&1; then
  echo "WESTON_XWAYLAND_FAIL: Xwayland is not installed" >&2
  exit 1
fi

weston_pid=
smoke_pid=
cleanup() {
  if [[ -n "${smoke_pid}" ]]; then
    kill "${smoke_pid}" 2>/dev/null || true
    wait "${smoke_pid}" 2>/dev/null || true
  fi
  if [[ -n "${weston_pid}" ]]; then
    kill "${weston_pid}" 2>/dev/null || true
    wait "${weston_pid}" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

export XDG_RUNTIME_DIR="${runtime_dir}"
unset DISPLAY
rm -f "${runtime_dir}/${wayland_socket}" "${runtime_dir}/${wayland_socket}.lock"

# Weston 13 accepts --renderer=pixman; older supported distributions use
# --use-pixman. Start with the modern spelling and retry only when Weston exits
# before publishing its Wayland socket.
start_weston() {
  : > "${weston_log}"
  weston \
    --backend=headless-backend.so \
    --socket="${wayland_socket}" \
    --xwayland \
    --idle-time=0 \
    --width=1280 \
    --height=800 \
    "$@" \
    > "${weston_log}" 2>&1 &
  weston_pid=$!
}

weston_major="$({ weston --version 2>/dev/null || true; } | sed -n 's/[^0-9]*\([0-9][0-9]*\).*/\1/p' | head -n 1)"
if [[ -n "${weston_major}" && "${weston_major}" -lt 13 ]]; then
  start_weston --use-pixman
else
  start_weston --renderer=pixman
fi
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
  start_weston --use-pixman
fi

for _ in {1..80}; do
  [[ -S "${runtime_dir}/${wayland_socket}" ]] && break
  if ! kill -0 "${weston_pid}" 2>/dev/null; then
    echo "WESTON_XWAYLAND_FAIL: Weston exited before publishing its socket" >&2
    cat "${weston_log}" >&2
    exit 1
  fi
  sleep 0.25
done
if [[ ! -S "${runtime_dir}/${wayland_socket}" ]]; then
  echo "WESTON_XWAYLAND_FAIL: timed out waiting for ${runtime_dir}/${wayland_socket}" >&2
  cat "${weston_log}" >&2
  exit 1
fi

# Weston's XWM writes the allocated X display to its log. Do not guess :0: a
# shared CI host may already have X sockets and Xwayland intentionally chooses a
# free display number.
x_display=
for _ in {1..80}; do
  x_display="$({
    sed -n 's/.*xserver listening on display \(:[0-9][0-9]*\).*/\1/p' "${weston_log}" || true
    sed -n 's/.*Xwayland.*display \(:[0-9][0-9]*\).*/\1/p' "${weston_log}" || true
  } | tail -n 1)"
  [[ -n "${x_display}" ]] && break
  if ! kill -0 "${weston_pid}" 2>/dev/null; then
    echo "WESTON_XWAYLAND_FAIL: Weston exited while starting Xwayland" >&2
    cat "${weston_log}" >&2
    exit 1
  fi
  sleep 0.25
done
if [[ -z "${x_display}" ]]; then
  echo "WESTON_XWAYLAND_FAIL: Weston did not advertise its Xwayland display" >&2
  cat "${weston_log}" >&2
  exit 1
fi

display_number="${x_display#:}"
for _ in {1..40}; do
  [[ -S "/tmp/.X11-unix/X${display_number}" ]] && break
  sleep 0.25
done
if [[ ! -S "/tmp/.X11-unix/X${display_number}" ]]; then
  echo "WESTON_XWAYLAND_FAIL: X socket /tmp/.X11-unix/X${display_number} is missing" >&2
  cat "${weston_log}" >&2
  exit 1
fi

export WAYLAND_DISPLAY="${wayland_socket}"
export DISPLAY="${x_display}"
export GDK_BACKEND=x11
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export KAEL_LINUX_BACKEND=x11
unset KAEL_HEADLESS

cd "${repo_root}"
KAEL_WEBVIEW_SMOKE_REQUIRE_POINTER_LOCK=1 cargo run --locked -p kael \
  --example webview_gtk4_platform_smoke \
  --no-default-features \
  --features webview-gtk4 \
  > "${smoke_log}" 2>&1 &
smoke_pid=$!

# Keep the same GTK/WebKit application connected while it proves XI2 event
# selection, the real X pointer grab, synchronous lock state, and cleanup. A
# headless compositor has no physical motion source; injecting one from a
# second client also crashes Xwayland 23.2.6 on Ubuntu's ARM image, so motion
# decoding is covered deterministically below the runtime boundary.
wait "${smoke_pid}"
smoke_pid=
cat "${smoke_log}"

grep -q '^WEBVIEW_SMOKE_BACKEND: X11$' "${smoke_log}"
grep -q '^WEBVIEW_SMOKE_STAGE: retained-scene-png$' "${smoke_log}"
grep -q '^WEBVIEW_SMOKE_STAGE: gsk-gpu-specs ' "${smoke_log}"
grep -q '^WEBVIEW_SMOKE_STAGE: idle-event-driven$' "${smoke_log}"
grep -q '^WEBVIEW_SMOKE_STAGE: raw-platform-handles-X11$' "${smoke_log}"
grep -q '^WEBVIEW_SMOKE_STAGE: native-pointer-lock$' "${smoke_log}"
grep -q '^WEBVIEW_SMOKE_STAGE: pointer-lock-acquired$' "${smoke_log}"
grep -q '^WEBVIEW_SMOKE_STAGE: pointer-lock-acquire-and-release$' "${smoke_log}"
grep -q '^WEBVIEW_SMOKE_STAGE: native-context-menu$' "${smoke_log}"
grep -q '^WEBVIEW_SMOKE_STAGE: custom-protocol$' "${smoke_log}"
grep -q '^WEBVIEW_SMOKE_OK: .*url=kael-smoke://assets/probe' "${smoke_log}"
if grep -q 'RefCell already borrowed' "${smoke_log}"; then
  echo "WESTON_XWAYLAND_FAIL: synchronous GTK callback re-entered borrowed Kael state" >&2
  exit 1
fi
echo "WESTON_XWAYLAND_GTK4_OK: WAYLAND_DISPLAY=${wayland_socket} DISPLAY=${x_display}"
