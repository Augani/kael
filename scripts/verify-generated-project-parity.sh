#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target_dir="${workspace_dir}/target"
evidence_dir="${target_dir}/generated-project-parity-evidence"
python_bin="${KAEL_PLAYWRIGHT_PYTHON:-python3}"
http_port="${KAEL_GENERATED_PARITY_PORT:-8162}"
temporary_dir=""
http_pid=""

cleanup() {
  if [[ -n "${http_pid}" ]]; then
    kill "${http_pid}" 2>/dev/null || true
    wait "${http_pid}" 2>/dev/null || true
  fi
  if [[ -n "${temporary_dir}" && \
        "${temporary_dir}" == "${target_dir}"/generated-project-parity.* ]]; then
    find "${temporary_dir}" -depth -delete
  fi
}
trap cleanup EXIT

sha256_file() {
  "${python_bin}" -c \
    'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1], "rb").read()).hexdigest())' \
    "$1"
}

if ! "${python_bin}" -c 'import playwright' >/dev/null 2>&1; then
  echo "Playwright 1.62.0 is required for generated-project browser proof" >&2
  exit 2
fi
if ! command -v wasm-bindgen >/dev/null 2>&1; then
  echo "wasm-bindgen 0.2.122 is required for generated-project parity" >&2
  exit 2
fi
if ! command -v wasm-opt >/dev/null 2>&1; then
  echo "wasm-opt 132 is required for generated-project parity" >&2
  exit 2
fi

mkdir -p "${target_dir}" "${evidence_dir}"
find "${evidence_dir}" -mindepth 1 -depth -delete
temporary_dir="$(mktemp -d "${target_dir}/generated-project-parity.XXXXXX")"
cp "${workspace_dir}/scripts/fixtures/generated-project-parity.Cargo.toml" \
  "${temporary_dir}/Cargo.toml"
cp "${workspace_dir}/Cargo.lock" "${temporary_dir}/Cargo.lock"

export CARGO_TARGET_DIR="${target_dir}"
cargo build --manifest-path "${workspace_dir}/Cargo.toml" -p kael-cli --bin kael
cli="${target_dir}/debug/kael"
if [[ ! -x "${cli}" ]]; then
  echo "generated-project parity could not find the Kael CLI at ${cli}" >&2
  exit 1
fi

(
  cd "${temporary_dir}"
  "${cli}" new kael-generated-parity
) | tee "${evidence_dir}/scaffold.log"

project_dir="${temporary_dir}/kael-generated-parity"
main_source="${project_dir}/src/main.rs"
manifest="${project_dir}/Cargo.toml"
main_snapshot="${evidence_dir}/generated-main.rs"
manifest_snapshot="${evidence_dir}/generated-Cargo.toml"
cp "${main_source}" "${main_snapshot}"
cp "${manifest}" "${manifest_snapshot}"
main_sha256="$(sha256_file "${main_source}")"
manifest_sha256="$(sha256_file "${manifest}")"
printf '%s  src/main.rs\n' "${main_sha256}" > "${evidence_dir}/main.sha256"
printf '%s  Cargo.toml\n' "${manifest_sha256}" > "${evidence_dir}/manifest.sha256"

cargo check --manifest-path "${manifest}" --bin kael-generated-parity \
  2>&1 | tee "${evidence_dir}/native-check.log"
cargo metadata --locked --manifest-path "${manifest}" --format-version 1 \
  > "${evidence_dir}/metadata.json"
cargo fetch --locked --manifest-path "${manifest}" --target wasm32-unknown-unknown \
  2>&1 | tee "${evidence_dir}/wasm-fetch.log"
cmp "${main_source}" "${main_snapshot}"
cmp "${manifest}" "${manifest_snapshot}"

(
  cd "${project_dir}"
  export CARGO_NET_OFFLINE=true
  "${cli}" web build --out-dir "${evidence_dir}/web"
) 2>&1 | tee "${evidence_dir}/web-build.log"
grep -Fq "Optimized WebAssembly with Binaryen 132:" \
  "${evidence_dir}/web-build.log"
for artifact in index.html app.js app_bg.wasm; do
  if [[ ! -s "${evidence_dir}/web/${artifact}" ]]; then
    echo "generated-project parity is missing web/${artifact}" >&2
    exit 1
  fi
done
bash "${workspace_dir}/scripts/verify-browser-artifact-budget.sh" \
  "${evidence_dir}/web/app_bg.wasm" "${evidence_dir}/web/app.js" \
  | tee "${evidence_dir}/artifact-budget.log"
cmp "${main_source}" "${main_snapshot}"
cmp "${manifest}" "${manifest_snapshot}"

"${python_bin}" -u -m http.server "${http_port}" --bind 127.0.0.1 \
  --directory "${evidence_dir}/web" > "${evidence_dir}/http.log" 2>&1 &
http_pid=$!
for attempt in {1..30}; do
  if curl --fail --silent "http://127.0.0.1:${http_port}/" >/dev/null; then
    break
  fi
  if ! kill -0 "${http_pid}" 2>/dev/null; then
    echo "generated-project HTTP server exited before becoming ready" >&2
    cat "${evidence_dir}/http.log" >&2
    exit 1
  fi
  if [[ "${attempt}" == 30 ]]; then
    echo "generated-project HTTP server did not become ready" >&2
    exit 1
  fi
  sleep 1
done

"${python_bin}" "${workspace_dir}/scripts/verify_generated_project_browser.py" \
  --url "http://127.0.0.1:${http_port}/" \
  --artifacts "${evidence_dir}" \
  --metadata "${evidence_dir}/metadata.json" \
  --workspace "${workspace_dir}" \
  --main-sha256 "${main_sha256}"

cmp "${main_source}" "${main_snapshot}"
cmp "${manifest}" "${manifest_snapshot}"
echo "Generated kael new native/browser parity passed; evidence: ${evidence_dir}"
