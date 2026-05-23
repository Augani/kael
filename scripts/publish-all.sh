#!/bin/bash
set -euo pipefail

CRATES=(
  # Tier 0 remaining
  kael_i18n
  kael_icons
  kael_net
  kael_notifications
  kael_pdf
  kael_release
  kael_share
  kael_storage
  kael_media_sys
  kael-media
  # Tier 1
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
)

publish_crate() {
  local crate="$1"
  while true; do
    echo "$(date -u +%H:%M:%S) Publishing $crate..."
    output=$(cargo publish -p "$crate" 2>&1) || true

    if echo "$output" | grep -q "Published\|already been uploaded"; then
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
  # Check if already published
  if cargo search "$crate" --limit 1 2>/dev/null | grep -q "^$crate "; then
    echo "$(date -u +%H:%M:%S) ⊘ $crate already published, skipping"
    continue
  fi

  publish_crate "$crate"
  echo ""
done

echo "=== All crates published! ==="
