#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "verify-macos-native-text-accessibility.sh requires macOS" >&2
  exit 2
fi

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
evidence_dir="${workspace_dir}/target/native-accessibility-smoke/macos-text"
mkdir -p "${evidence_dir}"
unset KAEL_HEADLESS
export KAEL_NATIVE_TEXT_SELF_TEST=1
export KAEL_NATIVE_TEXT_SMOKE=1
export CARGO_TARGET_DIR="${workspace_dir}/target"

(
  cd "${workspace_dir}"
  cargo run -p kael_ui --example editor_accessibility \
    --no-default-features --features native,editor
) 2>&1 | tee "${evidence_dir}/editor-accessibility.log"

grep -Fq "NATIVE_TEXT_FIXTURE_READY:" "${evidence_dir}/editor-accessibility.log"
grep -Fq "NATIVE_TEXT_ACCESSIBILITY_OK platform=macos" "${evidence_dir}/editor-accessibility.log"
grep -Fq "NATIVE_TEXT_FIXTURE_COMPLETE" "${evidence_dir}/editor-accessibility.log"
echo "macOS real Editor native text protocol proof passed; evidence: ${evidence_dir}"
