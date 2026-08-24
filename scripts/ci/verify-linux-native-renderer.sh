#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "verify-linux-native-renderer.sh requires Linux" >&2
  exit 2
fi

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
target_dir="${workspace_dir}/target"
evidence_dir="${target_dir}/native-renderer-smoke/linux"
display_number="${KAEL_NATIVE_RENDERER_DISPLAY:-:97}"
xvfb_pid=""
generated_pid=""
generated_root=""

cleanup() {
  if [[ -n "${generated_pid}" ]] && kill -0 "${generated_pid}" 2>/dev/null; then
    kill "${generated_pid}" 2>/dev/null || true
    wait "${generated_pid}" 2>/dev/null || true
  fi
  if [[ -n "${xvfb_pid}" ]] && kill -0 "${xvfb_pid}" 2>/dev/null; then
    kill "${xvfb_pid}" 2>/dev/null || true
    wait "${xvfb_pid}" 2>/dev/null || true
  fi
  if [[ -n "${generated_root}" && \
        "${generated_root}" == "${target_dir}"/generated-native-runtime.* ]]; then
    find "${generated_root}" -depth -delete
  fi
}
trap cleanup EXIT

for command in Xvfb xdpyinfo xdotool xwd; do
  if ! command -v "${command}" >/dev/null 2>&1; then
    echo "${command} is required for the Linux native renderer proof" >&2
    exit 2
  fi
done

mkdir -p "${target_dir}" "${evidence_dir}"
find "${evidence_dir}" -mindepth 1 -depth -delete

unset KAEL_HEADLESS WAYLAND_DISPLAY
export KAEL_LINUX_BACKEND=x11
export DISPLAY="${display_number}"
export CARGO_TARGET_DIR="${target_dir}"

if [[ "${KAEL_NATIVE_RENDERER_USE_SOFTWARE:-0}" == "1" ]]; then
  lvp_manifest=""
  for candidate in \
    /usr/share/vulkan/icd.d/lvp_icd.x86_64.json \
    /usr/share/vulkan/icd.d/lvp_icd.aarch64.json \
    /usr/share/vulkan/icd.d/lvp_icd.json; do
    if [[ -f "${candidate}" ]]; then
      lvp_manifest="${candidate}"
      break
    fi
  done
  if [[ -z "${lvp_manifest}" ]]; then
    echo "lavapipe Vulkan ICD was requested but no lvp ICD manifest was found" >&2
    exit 2
  fi
  export LIBGL_ALWAYS_SOFTWARE=1
  export VK_DRIVER_FILES="${lvp_manifest}"
  # Older Vulkan loaders consume VK_ICD_FILENAMES instead of VK_DRIVER_FILES.
  export VK_ICD_FILENAMES="${lvp_manifest}"
  export KAEL_EXPECT_SOFTWARE_RENDERER=1
fi

Xvfb "${DISPLAY}" -screen 0 1280x800x24 -nolisten tcp \
  > "${evidence_dir}/xvfb.log" 2>&1 &
xvfb_pid=$!
for attempt in {1..50}; do
  if xdpyinfo -display "${DISPLAY}" >/dev/null 2>&1; then
    break
  fi
  if ! kill -0 "${xvfb_pid}" 2>/dev/null; then
    echo "Xvfb exited before the native renderer proof started" >&2
    cat "${evidence_dir}/xvfb.log" >&2
    exit 1
  fi
  if [[ "${attempt}" == 50 ]]; then
    echo "Xvfb did not become ready" >&2
    exit 1
  fi
  sleep 0.1
done

{
  echo "os=$(uname -srmo)"
  echo "display=${DISPLAY}"
  echo "kael_linux_backend=${KAEL_LINUX_BACKEND}"
  echo "software_requested=${KAEL_NATIVE_RENDERER_USE_SOFTWARE:-0}"
  echo "vulkan_icd=${VK_DRIVER_FILES:-automatic}"
  xdpyinfo -display "${DISPLAY}" | sed -n '1,12p'
} > "${evidence_dir}/environment.txt"
if command -v vulkaninfo >/dev/null 2>&1; then
  vulkaninfo --summary > "${evidence_dir}/vulkan-summary.txt" 2>&1 || true
fi

export KAEL_NATIVE_RENDERER_SMOKE_PNG="${evidence_dir}/native-renderer.png"
(
  cd "${workspace_dir}"
  timeout --preserve-status 45s cargo run -p kael \
    --example native_renderer_smoke \
    --no-default-features --features font-kit,x11,runtime_shaders
) 2>&1 | tee "${evidence_dir}/native-renderer.log"

grep -Fq "NATIVE_RENDERER_SMOKE_GPU: backend=blade-vulkan" \
  "${evidence_dir}/native-renderer.log"
grep -Fq "NATIVE_RENDERER_SMOKE_OK:" "${evidence_dir}/native-renderer.log"
grep -Fq "text_probe_pixels=" "${evidence_dir}/native-renderer.log"
test -s "${evidence_dir}/native-renderer.png"

if [[ "${KAEL_SKIP_GENERATED_NATIVE_RUNTIME:-0}" == "1" ]]; then
  echo "GENERATED_NATIVE_RUNTIME_SKIPPED: explicitly disabled" \
    | tee "${evidence_dir}/generated-native.log"
  exit 0
fi

cargo build --manifest-path "${workspace_dir}/Cargo.toml" -p kael-cli --bin kael
cli="${target_dir}/debug/kael"
if [[ ! -x "${cli}" ]]; then
  echo "Kael CLI was not built at ${cli}" >&2
  exit 1
fi

generated_root="$(mktemp -d "${target_dir}/generated-native-runtime.XXXXXX")"
cp "${workspace_dir}/scripts/fixtures/generated-project-parity.Cargo.toml" \
  "${generated_root}/Cargo.toml"
cp "${workspace_dir}/Cargo.lock" "${generated_root}/Cargo.lock"
(
  cd "${generated_root}"
  "${cli}" new kael-generated-parity
) 2>&1 | tee "${evidence_dir}/generated-scaffold.log"

generated_project="${generated_root}/kael-generated-parity"
generated_main="${generated_project}/src/main.rs"
generated_manifest="${generated_project}/Cargo.toml"
cp "${generated_main}" "${evidence_dir}/generated-main.rs"
cp "${generated_manifest}" "${evidence_dir}/generated-Cargo.toml"
sha256sum "${generated_main}" > "${evidence_dir}/generated-main-before.sha256"

cargo build --manifest-path "${generated_manifest}" \
  --bin kael-generated-parity \
  2>&1 | tee "${evidence_dir}/generated-build.log"
generated_binary="${target_dir}/debug/kael-generated-parity"
if [[ ! -x "${generated_binary}" ]]; then
  echo "generated native binary was not built at ${generated_binary}" >&2
  exit 1
fi

"${generated_binary}" \
  > "${evidence_dir}/generated-app.stdout.log" \
  2> "${evidence_dir}/generated-app.stderr.log" &
generated_pid=$!
generated_window=""
for attempt in {1..200}; do
  if ! kill -0 "${generated_pid}" 2>/dev/null; then
    wait "${generated_pid}" || true
    generated_pid=""
    echo "generated project exited before mapping its native window" >&2
    cat "${evidence_dir}/generated-app.stderr.log" >&2
    exit 1
  fi
  generated_window="$(
    xdotool search --onlyvisible --pid "${generated_pid}" \
      --name '^kael-generated-parity$' 2>/dev/null | head -n 1 || true
  )"
  if [[ -n "${generated_window}" ]]; then
    break
  fi
  if [[ "${attempt}" == 200 ]]; then
    echo "generated project did not map a visible native window" >&2
    exit 1
  fi
  sleep 0.05
done

xdotool getwindowgeometry --shell "${generated_window}" \
  > "${evidence_dir}/generated-window-geometry.txt"
xwininfo -id "${generated_window}" \
  > "${evidence_dir}/generated-window-xinfo.txt"
xwd -silent -id "${generated_window}" \
  -out "${evidence_dir}/generated-window.xwd"
test "$(wc -c < "${evidence_dir}/generated-window.xwd")" -gt 1024

# The unchanged starter intentionally runs until its window is closed. CI uses
# an explicit bounded external stop after proving the native window is mapped;
# the self-terminating renderer example above is the clean lifecycle gate.
kill "${generated_pid}"
set +e
wait "${generated_pid}"
generated_status=$?
set -e
generated_pid=""
if [[ "${generated_status}" -ne 143 && "${generated_status}" -ne 0 ]]; then
  echo "generated native app exited with unexpected status ${generated_status}" >&2
  exit 1
fi

sha256sum "${generated_main}" > "${evidence_dir}/generated-main-after.sha256"
cmp "${generated_main}" "${evidence_dir}/generated-main.rs"
cmp "${generated_manifest}" "${evidence_dir}/generated-Cargo.toml"
cmp "${evidence_dir}/generated-main-before.sha256" \
  "${evidence_dir}/generated-main-after.sha256"
{
  echo "GENERATED_NATIVE_RUNTIME_WINDOW: id=${generated_window} pid_stopped=${generated_status}"
  sed -n -e 's/^WIDTH=/width=/p' -e 's/^HEIGHT=/height=/p' \
    "${evidence_dir}/generated-window-geometry.txt"
  echo "GENERATED_NATIVE_RUNTIME_OK: unchanged CLI project built and mapped a real X11 window"
} | tee "${evidence_dir}/generated-native.log"

echo "Linux Blade release proof passed; evidence: ${evidence_dir}"
