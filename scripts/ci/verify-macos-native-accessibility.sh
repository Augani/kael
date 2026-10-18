#!/usr/bin/env bash
set -euo pipefail
if [[ "$(uname -s)" != Darwin ]]; then
  echo "Native NSAccessibility verification requires macOS" >&2
  exit 2
fi
workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
evidence_dir="${workspace_dir}/target/native-accessibility-smoke/macos"
mkdir -p "${evidence_dir}"
cd "${workspace_dir}"
sw_vers > "${evidence_dir}/environment.txt"
cargo test --locked -p kael_accesskit_macos --lib --test native_outline --test native_text_selection \
  -- --nocapture 2>&1 | tee "${evidence_dir}/native-outline.log"
grep -Fq 'native NSAccessibility: 100025 rows, 4000 disclosed children' "${evidence_dir}/native-outline.log"
grep -Fq 'native NSAccessibility text selection: full multiline value' "${evidence_dir}/native-outline.log"
