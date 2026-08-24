#!/usr/bin/env bash
set -euo pipefail

# The maintained `webview` feature uses GTK4/WebKitGTK 6 on both Wayland and
# X11 and must never acquire an archived GTK3/Wry graph. The remaining accepted
# Wry is Windows-only in Kael, but Cargo.lock records Wry's target-conditional
# GTK3 dependencies. The graph checks below prove neither portable WebView
# spelling can reach them on Linux. The remaining advisories are unmaintained
# transitive parser/build crates without a published vulnerability. Keep the
# list explicit so every new warning fails CI instead of silently joining it.
accepted_advisories=(
  RUSTSEC-2024-0412
  RUSTSEC-2024-0413
  RUSTSEC-2024-0414
  RUSTSEC-2024-0415
  RUSTSEC-2024-0416
  RUSTSEC-2024-0417
  RUSTSEC-2024-0418
  RUSTSEC-2024-0419
  RUSTSEC-2024-0420
  RUSTSEC-2024-0429
  RUSTSEC-2024-0436
  RUSTSEC-2024-0370
  RUSTSEC-2026-0192
)

ignore_args=()
for advisory in "${accepted_advisories[@]}"; do
  ignore_args+=(--ignore "${advisory}")
done

cargo audit -D warnings "${ignore_args[@]}"

if [[ "$(uname -s)" == "Linux" ]]; then
  for feature in webview webview-legacy-gtk3; do
    maintained_webview_tree="$(
      cargo tree --locked -p kael --no-default-features --features "${feature}" \
        --target "$(rustc -vV | sed -n 's/^host: //p')" --edges normal,build
    )"
    if grep -Eq 'gtk v0\.18|webkit2gtk|wry v0\.56|blade-graphics' \
      <<< "${maintained_webview_tree}"; then
      echo "DEPENDENCY_AUDIT_FAIL: Linux ${feature} graph contains a legacy/raw host" >&2
      grep -E 'gtk v0\.18|webkit2gtk|wry v0\.56|blade-graphics' \
        <<< "${maintained_webview_tree}" >&2
      exit 1
    fi
  done
  echo "DEPENDENCY_AUDIT_OK: portable and deprecated-alias Linux WebView graphs are GTK4/WebKitGTK6-only"
fi
