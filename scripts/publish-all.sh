#!/usr/bin/env bash
set -euo pipefail

mode="${1:---preflight}"
case "$mode" in
  --preflight|--dry-run)
    mode="preflight"
    ;;
  --execute)
    mode="execute"
    ;;
  *)
    echo "usage: scripts/publish-all.sh [--preflight|--execute]" >&2
    exit 2
    ;;
esac

# Dependency-first order. Keep this list explicit so a release review shows
# exactly what will be uploaded and in what sequence.
crates=(
  kael_sum_tree
  kael_storage
  kael_share
  kael_semantic_version
  kael_secrets
  kael_render_graph
  kael_release
  kael_pdf
  kael_notifications
  kael_media_sys
  kael_icons
  kael_i18n
  kael_gpu_budget
  kael_engines
  kael_document
  kael_derive_refineable
  kael_collections
  kael_cache
  kael-media
  kael-macros
  kael-cli
  kael_net
  kael_media_engines
  kael_refineable
  kael_perf
  kael_audio
  kael_util_macros
  kael_util
  kael_http_client
  kael_diagnostics
  kael
  kael_ui
)

package_version="$(cargo pkgid -p kael | sed -E 's/.*[@#]//')"

preflight_crate() {
  local crate="$1"
  local listing
  listing="$(cargo package --locked --allow-dirty -p "$crate" --list)"
  if grep -Eq '(^|/)(examples?|benches?)/' <<<"$listing"; then
    echo "error: $crate would publish an example or benchmark" >&2
    return 1
  fi
  echo "package contents clean: $crate ($(wc -l <<<"$listing" | tr -d ' ') files)"
}

registry_has_version() {
  local crate="$1"
  cargo info "$crate@$package_version" --registry crates-io >/dev/null 2>&1
}

wait_for_registry() {
  local crate="$1"
  local attempt
  for attempt in {1..18}; do
    if registry_has_version "$crate"; then
      return 0
    fi
    sleep 10
  done
  echo "error: $crate@$package_version did not appear in the crates.io index" >&2
  return 1
}

publish_crate() {
  local crate="$1"
  if registry_has_version "$crate"; then
    echo "skip: $crate@$package_version already exists"
    return 0
  fi

  cargo publish --locked -p "$crate"
  wait_for_registry "$crate"
}

if [[ "$mode" == "execute" ]]; then
  if [[ "${KAEL_PUBLISH_CONFIRM:-}" != "publish-kael-$package_version" ]]; then
    echo "error: set KAEL_PUBLISH_CONFIRM=publish-kael-$package_version to publish" >&2
    exit 2
  fi
  if [[ -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
    echo "error: CARGO_REGISTRY_TOKEN is required" >&2
    exit 2
  fi
  if [[ -n "$(git status --porcelain)" ]]; then
    echo "error: refusing to publish from a dirty worktree" >&2
    exit 2
  fi
fi

echo "Kael $package_version crate ${mode}: ${#crates[@]} packages"
for crate in "${crates[@]}"; do
  echo "==> $crate"
  if [[ "$mode" == "preflight" ]]; then
    preflight_crate "$crate"
  else
    publish_crate "$crate"
  fi
done

if [[ "$mode" == "preflight" ]]; then
  echo "Preflight complete. No crates were uploaded."
else
  echo "Published all Kael $package_version crates."
fi
