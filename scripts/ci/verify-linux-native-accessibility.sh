#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != Linux ]]; then
  echo "Native AT-SPI verification requires Linux" >&2
  exit 2
fi
workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
profile=x11
session=false
text_protocol=false
for option in "$@"; do
  case "${option}" in
    --gtk4) profile=gtk4 ;;
    --session) session=true ;;
    --text) text_protocol=true ;;
    *) echo "Unknown native accessibility option: ${option}" >&2; exit 2 ;;
  esac
done
evidence_name=linux
features=native,kael/font-kit,kael/x11
session_options=()
example_name=virtual_tree
client_script=scripts/ci/native-accessibility-atspi.py
success_marker='NATIVE_ACCESSIBILITY_RUNTIME_OK: backend=atspi'
if [[ "${profile}" == gtk4 ]]; then
  evidence_name=linux-gtk4
  features=native,kael/webview-wayland-gtk4
  session_options+=(--gtk4)
fi
if [[ "${text_protocol}" == true ]]; then
  evidence_name+=-text
  features+=,editor
  example_name=editor_accessibility
  client_script=scripts/ci/native-text-accessibility-atspi.py
  success_marker='NATIVE_TEXT_ACCESSIBILITY_OK platform=linux'
  session_options+=(--text)
fi
evidence_dir="${workspace_dir}/target/native-accessibility-smoke/${evidence_name}"
mkdir -p "${evidence_dir}"
cd "${workspace_dir}"

if [[ "${session}" == false ]]; then
  cargo build --locked -p kael_ui --example "${example_name}" --no-default-features \
    --features "${features}" \
    2>&1 | tee "${evidence_dir}/build.log"
  # The accessibility bus/status and X server belong solely to this CI session.
  exec dbus-run-session -- xvfb-run -a -s '-screen 0 1280x800x24 -nolisten tcp' \
    bash "${BASH_SOURCE[0]}" --session "${session_options[@]}"
fi

unset KAEL_HEADLESS WAYLAND_DISPLAY
export KAEL_LINUX_BACKEND=x11
export KAEL_ATSPI_TRACE=1
unset KAEL_ACCESSIBILITY_SMOKE KAEL_NATIVE_TEXT_SMOKE
if [[ "${text_protocol}" == true ]]; then
  export KAEL_NATIVE_TEXT_SMOKE=1
else
  export KAEL_ACCESSIBILITY_SMOKE=1
fi
if [[ "${profile}" == gtk4 ]]; then
  export GDK_BACKEND=x11 GSK_RENDERER=cairo
elif [[ "${KAEL_NATIVE_RENDERER_USE_SOFTWARE:-0}" == 1 ]]; then
  lvp_manifest=""
  for candidate in /usr/share/vulkan/icd.d/lvp_icd.x86_64.json \
    /usr/share/vulkan/icd.d/lvp_icd.aarch64.json /usr/share/vulkan/icd.d/lvp_icd.json; do
    if [[ -f "${candidate}" ]]; then lvp_manifest="${candidate}"; break; fi
  done
  if [[ -z "${lvp_manifest}" ]]; then
    echo "Requested Lavapipe ICD is unavailable" >&2
    exit 2
  fi
  export VK_DRIVER_FILES="${lvp_manifest}" VK_ICD_FILENAMES="${lvp_manifest}"
  export LIBGL_ALWAYS_SOFTWARE=1
fi
gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus \
  --method org.freedesktop.DBus.Properties.Set org.a11y.Status IsEnabled '<true>' \
  > "${evidence_dir}/bus-status.log"
bus_reply="$(gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus \
  --method org.a11y.Bus.GetAddress)"
AT_SPI_BUS_ADDRESS="$(python3 -c 'import ast,sys; print(ast.literal_eval(sys.argv[1])[0])' "${bus_reply}")"
export AT_SPI_BUS_ADDRESS
{
  echo "os=$(uname -srmo)"
  echo "display=${DISPLAY}"
  echo "profile=${profile}"
  echo "fixture=${example_name}"
  echo "renderer=${GSK_RENDERER:-${VK_DRIVER_FILES:-automatic}}"
  echo "client=AT-SPI2 D-Bus through GLib introspection"
} > "${evidence_dir}/environment.txt"

owned_pid=""
cleanup() {
  if [[ -n "${owned_pid}" ]] && kill -0 "${owned_pid}" 2>/dev/null; then
    kill "${owned_pid}" 2>/dev/null || true
    wait "${owned_pid}" 2>/dev/null || true
  fi
}
trap cleanup EXIT
# Confirm the owned X server accepts an authenticated client before launching
# the fixture, and preserve its extensions/display details on startup failure.
xdpyinfo > "${evidence_dir}/x-server.txt"
app_log="${evidence_dir}/${example_name}.log"
client_log="${evidence_dir}/atspi-client.log"
"${workspace_dir}/target/debug/examples/${example_name}" \
  > "${app_log}" 2> "${evidence_dir}/${example_name}.stderr.log" &
owned_pid=$!
# This Xvfb session has no window manager. Establish real X keyboard focus for
# the owned visible host, as a desktop window manager normally does on launch.
# Logical control focus must never substitute for an inactive native host.
owned_window=""
for ((attempt=0; attempt<300; attempt++)); do
  owned_window="$(xdotool search --onlyvisible --pid "${owned_pid}" 2>/dev/null | head -n 1 || true)"
  if [[ -n "${owned_window}" ]]; then break; fi
  if ! kill -0 "${owned_pid}" 2>/dev/null; then
    echo 'Owned accessibility application exited before showing its window' >&2
    exit 1
  fi
  sleep 0.05
done
if [[ -z "${owned_window}" ]]; then
  echo 'Owned accessibility application did not show a native window' >&2
  exit 1
fi
if [[ "$(xdotool getwindowpid "${owned_window}")" != "${owned_pid}" ]]; then
  echo 'Native accessibility host window belongs to a different process' >&2
  exit 1
fi
timeout 10s xdotool windowfocus --sync "${owned_window}"
echo "NATIVE_ACCESSIBILITY_HOST_FOCUS: pid=${owned_pid} window=${owned_window}" | tee -a "${evidence_dir}/environment.txt"
client_options=(--pid "${owned_pid}" --app-log "${app_log}")
if [[ "${text_protocol}" == true ]]; then
  client_options+=(
    --document crates/kael_ui/examples/fixtures/native_unicode_document.txt
    --replacement crates/kael_ui/examples/fixtures/native_unicode_replacement.txt
  )
fi
# Use the distribution Python that owns python3-gi, even if setup-python changed PATH.
timeout --preserve-status 105s /usr/bin/python3 "${client_script}" \
  "${client_options[@]}" 2>&1 | tee "${client_log}"
grep -Fq "${success_marker}" "${client_log}"
if [[ "${text_protocol}" == true ]]; then
  # A client result cannot hide the fixture deadline or a failed final action.
  for ((attempt=0; attempt<100; attempt++)); do
    if ! kill -0 "${owned_pid}" 2>/dev/null; then break; fi
    sleep 0.1
  done
  if kill -0 "${owned_pid}" 2>/dev/null; then
    echo 'Native text fixture did not exit after successful client completion' >&2
    exit 1
  fi
  wait "${owned_pid}"
  grep -Fq 'NATIVE_TEXT_FIXTURE_COMPLETE' "${app_log}"
fi
