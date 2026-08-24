#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "verify-macos-native-renderer.sh requires macOS" >&2
  exit 2
fi

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
evidence_dir="${workspace_dir}/target/native-renderer-smoke/macos"
mkdir -p "${evidence_dir}"
find "${evidence_dir}" -mindepth 1 -depth -delete

unset KAEL_HEADLESS
export KAEL_NATIVE_RENDERER_SMOKE_PNG="${evidence_dir}/native-renderer.png"
export CARGO_TARGET_DIR="${workspace_dir}/target"

(
  cd "${workspace_dir}"
  cargo run -p kael --example native_renderer_smoke \
    --no-default-features --features font-kit,runtime_shaders
) 2>&1 | tee "${evidence_dir}/native-renderer.log"

grep -Fq "NATIVE_RENDERER_SMOKE_GPU: backend=metal software=false" \
  "${evidence_dir}/native-renderer.log"
grep -Fq "text_probe_pixels=" "${evidence_dir}/native-renderer.log"
grep -Fq "NATIVE_RENDERER_SMOKE_OK:" "${evidence_dir}/native-renderer.log"
test -s "${evidence_dir}/native-renderer.png"

echo "macOS Metal release proof passed; evidence: ${evidence_dir}"
