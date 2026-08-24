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
  kael_office
  kael_notifications
  kael_media_sys
  kael_markdown
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
package_minor_version="${package_version%.*}"
dist_version="$(sed -nE 's/^version[[:space:]]*=[[:space:]]*"([^"]+)"/\1/p' kael.dist.toml | head -n 1)"
scaffold_version="$(sed -nE 's/^const KAEL_VERSION: &str = "([^"]+)";/\1/p' xtask/src/scaffold.rs | head -n 1)"

if [[ "$dist_version" != "$package_version" ]]; then
  echo "error: kael.dist.toml version $dist_version does not match workspace $package_version" >&2
  exit 1
fi
if [[ "$scaffold_version" != "$package_minor_version" ]]; then
  echo "error: scaffold dependency version $scaffold_version does not match workspace minor $package_minor_version" >&2
  exit 1
fi
if ! grep -Eq "^## \\[$package_version\\] - [0-9]{4}-[0-9]{2}-[0-9]{2}$" CHANGELOG.md; then
  echo "error: CHANGELOG.md has no dated [$package_version] release section" >&2
  exit 1
fi

preflight_crate() {
  local crate="$1"
  local crate_version
  local listing
  local unapproved_examples
  crate_version="$(cargo pkgid -p "$crate" | sed -E 's/.*[@#]//')"
  if [[ "$crate_version" != "$package_version" ]]; then
    echo "error: $crate version $crate_version does not match workspace $package_version" >&2
    return 1
  fi
  listing="$(cargo package --locked --allow-dirty -p "$crate" --list)"

  if ! grep -qx 'LICENSE-APACHE' <<<"$listing"; then
    echo "error: $crate would publish without its Apache-2.0 license text" >&2
    return 1
  fi
  unapproved_examples="$(grep -E '(^|/)(examples?|benches?)/' <<<"$listing" || true)"
  if [[ "$crate" == "kael_net" ]]; then
    for required_example in \
      examples/browser_websocket_smoke.rs \
      examples/websocket_echo_server.rs; do
      if ! grep -qx "$required_example" <<<"$listing"; then
        echo "error: $crate would publish without declared example $required_example" >&2
        return 1
      fi
    done
    unapproved_examples="$(grep -Ev \
      '^(examples/browser_websocket_smoke\.rs|examples/websocket_echo_server\.rs)$' \
      <<<"$unapproved_examples" || true)"
  fi
  if [[ -n "$unapproved_examples" ]]; then
    echo "error: $crate would publish an unapproved example or benchmark:" >&2
    printf '%s\n' "$unapproved_examples" >&2
    return 1
  fi

  if grep -Eq '^assets/fonts/Inter-.*\.ttf$' <<<"$listing" &&
    ! grep -qx 'assets/fonts/LICENSE-INTER' <<<"$listing"; then
    echo "error: $crate would publish Inter fonts without their OFL-1.1 notice" >&2
    return 1
  fi
  if grep -Eq '^assets/fonts/JetBrainsMono-.*\.ttf$' <<<"$listing" &&
    ! grep -qx 'assets/fonts/LICENSE-JETBRAINS-MONO' <<<"$listing"; then
    echo "error: $crate would publish JetBrains Mono without its OFL-1.1 notice" >&2
    return 1
  fi
  if grep -Eq '^assets/fonts/.*\.ttf$' <<<"$listing" &&
    ! grep -qx 'THIRD_PARTY_LICENSES.md' <<<"$listing"; then
    echo "error: $crate would publish fonts without a third-party license index" >&2
    return 1
  fi
  if grep -Eq '^icons/.*\.svg$' <<<"$listing" &&
    ! grep -qx 'LICENSE-LUCIDE' <<<"$listing"; then
    echo "error: $crate would publish Lucide icons without their ISC/MIT notice" >&2
    return 1
  fi
  if grep -Eq '^icons/.*\.svg$' <<<"$listing" &&
    ! grep -qx 'THIRD_PARTY_LICENSES.md' <<<"$listing"; then
    echo "error: $crate would publish icons without a third-party license index" >&2
    return 1
  fi
  echo "package contents clean: $crate ($(wc -l <<<"$listing" | tr -d ' ') files)"
}

verify_package_archives() {
  local crate
  local archive
  local archive_bytes
  local -a package_args=()
  local max_crate_bytes=10485760

  # Package the complete selected set at once. Cargo can then resolve workspace
  # dependencies that intentionally do not exist on crates.io until this release
  # is uploaded, and verifies each extracted archive rather than only compiling
  # the source checkout.
  for crate in "${crates[@]}"; do
    package_args+=(--package "$crate")
  done
  cargo package --locked --allow-dirty "${package_args[@]}"

  for crate in "${crates[@]}"; do
    archive="target/package/${crate}-${package_version}.crate"
    if [[ ! -s "$archive" ]]; then
      echo "error: cargo package did not create $archive" >&2
      return 1
    fi
    archive_bytes="$(wc -c < "$archive" | tr -d ' ')"
    if ((archive_bytes > max_crate_bytes)); then
      echo "error: $archive exceeds the crates.io 10 MiB archive limit: $archive_bytes bytes" >&2
      return 1
    fi
    echo "package archive verified: $crate ($archive_bytes bytes)"
  done
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
  publish_ref="${KAEL_PUBLISH_REF:-}"
  if [[ -z "$publish_ref" ]]; then
    publish_ref="$(git symbolic-ref --quiet --short HEAD || true)"
  fi
  if [[ "$publish_ref" != "main" && "$publish_ref" != "refs/heads/main" ]]; then
    echo "error: publishing is only allowed from main; got ${publish_ref:-detached HEAD}" >&2
    exit 2
  fi
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
  verify_package_archives
  echo "Preflight complete. No crates were uploaded."
else
  echo "Published all Kael $package_version crates."
fi
