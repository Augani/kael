#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != Linux ]]; then
  echo "Native AT-SPI verification requires Linux" >&2
  exit 2
fi
workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
profile=x11
session=false
for option in "$@"; do
  case "${option}" in
    --gtk4) profile=gtk4 ;;
    --session) session=true ;;
    *) echo "Unknown native accessibility option: ${option}" >&2; exit 2 ;;
  esac
done
evidence_name=linux
features=native,kael/font-kit,kael/x11
session_options=()
if [[ "${profile}" == gtk4 ]]; then
  evidence_name=linux-gtk4
  features=native,kael/webview-wayland-gtk4
  session_options+=(--gtk4)
fi
evidence_dir="${workspace_dir}/target/native-accessibility-smoke/${evidence_name}"
mkdir -p "${evidence_dir}"
cd "${workspace_dir}"

if [[ "${session}" == false ]]; then
  cargo build --locked -p kael_ui --example virtual_tree --no-default-features \
    --features "${features}" \
    2>&1 | tee "${evidence_dir}/build.log"
  # The accessibility bus/status and X server belong solely to this CI session.
  exec dbus-run-session -- xvfb-run -a -s '-screen 0 1280x800x24 -nolisten tcp' \
    bash "${BASH_SOURCE[0]}" --session "${session_options[@]}"
fi

unset KAEL_HEADLESS WAYLAND_DISPLAY
export KAEL_LINUX_BACKEND=x11 KAEL_ACCESSIBILITY_SMOKE=1
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
"${workspace_dir}/target/debug/examples/virtual_tree" \
  > "${evidence_dir}/virtual-tree.log" 2> "${evidence_dir}/virtual-tree.stderr.log" &
owned_pid=$!
# Use the distribution Python that owns python3-gi, even if setup-python changed PATH.
timeout --preserve-status 105s /usr/bin/python3 scripts/ci/native-accessibility-atspi.py \
  --pid "${owned_pid}" --app-log "${evidence_dir}/virtual-tree.log" \
  2>&1 | tee "${evidence_dir}/atspi-client.log"
grep -Fq 'NATIVE_ACCESSIBILITY_RUNTIME_OK: backend=atspi' "${evidence_dir}/atspi-client.log"
