#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(pwd)"
package=""
target_kind="bin"
target_name=""
features="browser"
profile="release"
output_dir="$(pwd)/dist"
output_name="app"
html_source=""

usage() {
  echo "usage: bash scripts/build-web.sh --package NAME (--bin NAME | --example NAME) [options]"
  echo "options: --workspace DIR --features LIST --profile NAME --out-dir DIR --out-name NAME --html FILE"
}

require_value() {
  if (($# < 2)) || [[ -z "${2:-}" ]]; then
    echo "missing value for $1" >&2
    usage >&2
    exit 2
  fi
}

while (($#)); do
  case "$1" in
    --workspace) require_value "$@"; workspace_dir="$2"; shift 2 ;;
    --package) require_value "$@"; package="$2"; shift 2 ;;
    --bin) require_value "$@"; target_kind="bin"; target_name="$2"; shift 2 ;;
    --example) require_value "$@"; target_kind="example"; target_name="$2"; shift 2 ;;
    --features) require_value "$@"; features="$2"; shift 2 ;;
    --profile) require_value "$@"; profile="$2"; shift 2 ;;
    --out-dir) require_value "$@"; output_dir="$2"; shift 2 ;;
    --out-name) require_value "$@"; output_name="$2"; shift 2 ;;
    --html) require_value "$@"; html_source="$2"; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ -z "${package}" || -z "${target_name}" ]]; then
  usage >&2
  exit 2
fi

required_bindgen="0.2.122"
if ! command -v wasm-bindgen >/dev/null 2>&1; then
  echo "wasm-bindgen CLI is required: cargo install wasm-bindgen-cli --version ${required_bindgen} --locked" >&2
  exit 1
fi
actual_bindgen="$(wasm-bindgen --version | awk '{print $2}')"
if [[ "${actual_bindgen}" != "${required_bindgen}" ]]; then
  echo "wasm-bindgen ${required_bindgen} is required (found ${actual_bindgen})" >&2
  exit 1
fi

required_wasm_opt="132"
if [[ "${profile}" == "release" ]]; then
  if ! command -v wasm-opt >/dev/null 2>&1; then
    echo "wasm-opt ${required_wasm_opt} is required for optimized release output" >&2
    echo "Install it with: npm install --global binaryen@132.0.0" >&2
    exit 1
  fi
  actual_wasm_opt="$(wasm-opt --version | awk '{print $NF}')"
  if [[ "${actual_wasm_opt}" != "${required_wasm_opt}" ]]; then
    echo "wasm-opt ${required_wasm_opt} is required (found ${actual_wasm_opt})" >&2
    echo "Install it with: npm install --global binaryen@132.0.0" >&2
    exit 1
  fi
fi

workspace_dir="$(cd "${workspace_dir}" && pwd)"
mkdir -p "${output_dir}"
output_dir="$(cd "${output_dir}" && pwd)"

build_args=(
  build
  --target wasm32-unknown-unknown
  --package "${package}"
  --no-default-features
  --features "${features}"
)
if [[ "${profile}" == "release" ]]; then
  build_args+=(--release)
  artifact_profile="release"
elif [[ "${profile}" == "dev" ]]; then
  artifact_profile="debug"
  build_args+=(--profile "${profile}")
else
  artifact_profile="${profile}"
  build_args+=(--profile "${profile}")
fi
if [[ "${target_kind}" == "example" ]]; then
  build_args+=(--example "${target_name}")
  artifact="${workspace_dir}/target/wasm32-unknown-unknown/${artifact_profile}/examples/${target_name}.wasm"
else
  build_args+=(--bin "${target_name}")
  artifact="${workspace_dir}/target/wasm32-unknown-unknown/${artifact_profile}/${target_name}.wasm"
fi

(cd "${workspace_dir}" && cargo "${build_args[@]}")
wasm-bindgen \
  --target web \
  --no-typescript \
  --out-dir "${output_dir}" \
  --out-name "${output_name}" \
  "${artifact}"

if [[ "${profile}" == "release" ]]; then
  packaged_wasm="${output_dir}/${output_name}_bg.wasm"
  optimized_wasm="${output_dir}/${output_name}_bg.optimized.wasm"
  before_bytes="$(wc -c < "${packaged_wasm}" | tr -d ' ')"
  wasm-opt -O3 "${packaged_wasm}" -o "${optimized_wasm}"
  mv "${optimized_wasm}" "${packaged_wasm}"
  after_bytes="$(wc -c < "${packaged_wasm}" | tr -d ' ')"
  echo "Optimized WebAssembly with Binaryen ${required_wasm_opt}: ${before_bytes} -> ${after_bytes} bytes"
fi

if [[ -n "${html_source}" ]]; then
  cp "${html_source}" "${output_dir}/index.html"
fi

echo "Built ${output_dir}/${output_name}.js and ${output_dir}/${output_name}_bg.wasm"
