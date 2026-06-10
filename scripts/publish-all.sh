#!/bin/bash
set -euo pipefail

DRY_RUN=0
if [[ "${1:-}" == "--dry-run" ]]; then
  DRY_RUN=1
fi

CRATES=(
  # Tier 0: independent crates
  kael_collections
  kael_derive_refineable
  kael_i18n
  kael_icons
  kael_semantic_version
  kael_sum_tree
  kael_net
  kael_notifications
  kael_pdf
  kael_release
  kael_share
  kael_storage
  kael_cache
  kael_engines
  kael_media_sys
  kael-media
  # Tier 1: direct dependencies on tier 0 crates
  kael_refineable
  kael_audio
  kael_perf
  kael_document
  # Tier 2
  kael_util_macros
  # Tier 3
  kael_util
  # Tier 4
  kael_http_client
  # Tier 5
  kael_diagnostics
  # Tier 6
  kael-macros
  # Tier 7 (main crate)
  kael
  # Tier 8: depends on the main crate
  kael_ui
)

publish_crate() {
  local crate="$1"
  if [[ "$DRY_RUN" == "1" ]]; then
    echo "$(date -u +%H:%M:%S) Dry-run $crate..."
    if cargo publish --dry-run -p "$crate"; then
      echo "$(date -u +%H:%M:%S) ✓ $crate ok (dry-run)"
      return 0
    fi
    echo "$(date -u +%H:%M:%S) ✗ $crate FAILED (dry-run)"
    return 1
  fi
  while true; do
    echo "$(date -u +%H:%M:%S) Publishing $crate..."
    if output=$(cargo publish -p "$crate" 2>&1); then status=0; else status=$?; fi

    if [[ $status -eq 0 ]] || echo "$output" | grep -qiE "Uploaded |already (exists|been uploaded)|is already uploaded"; then
      echo "$(date -u +%H:%M:%S) ✓ $crate published"
      return 0
    fi

    if echo "$output" | grep -q "429 Too Many Requests"; then
      retry_after=$(echo "$output" | grep -oP 'after \K[^"]+(?= and)' || echo "")
      echo "$(date -u +%H:%M:%S) Rate limited. Retry after: $retry_after"
      echo "$(date -u +%H:%M:%S) Waiting 600s..."
      sleep 600
      continue
    fi

    echo "$(date -u +%H:%M:%S) ✗ $crate FAILED:"
    echo "$output" | tail -10
    return 1
  done
}

echo "=== Kael crates.io publish ==="
echo "$(date -u +%H:%M:%S) Starting with ${#CRATES[@]} crates"
echo ""

for crate in "${CRATES[@]}"; do
  publish_crate "$crate"
  echo ""
done

if [[ "$DRY_RUN" == "1" ]]; then
  echo "=== Dry-run complete (no crates uploaded) ==="
else
  echo "=== All crates published! ==="
fi
