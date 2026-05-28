#!/usr/bin/env bash
# Build the objc2_smoke example, wrap it in a minimal .app bundle so that
# macOS recognises a bundleIdentifier and accepts the URL-scheme
# registration, then launch it. After it's running, opening kael://hi in
# a browser should hand the URL to the app and surface it via the
# on_open_urls callback (printed to stderr) and the in-window event log.
#
# Re-run this script after any code change — it always rebuilds the
# binary and refreshes the bundle, so you don't need to clear caches.
#
# Usage:
#   scripts/run-objc2-smoke-bundled.sh           # build + run
#   scripts/run-objc2-smoke-bundled.sh --build   # build + refresh bundle, no launch

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

profile="${PROFILE:-debug}"
case "$profile" in
    debug) cargo_flag="" ;;
    release) cargo_flag="--release" ;;
    *) echo "PROFILE must be debug or release, got: $profile" >&2; exit 1 ;;
esac

bundle_id="com.kael.objc2-smoke"
bundle_name="Objc2Smoke"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
bin="$target_dir/$profile/examples/objc2_smoke"
bundle="$target_dir/$profile/examples/${bundle_name}.app"
contents="$bundle/Contents"

echo "==> Building example (profile=$profile)..."
cargo build -p kael --example objc2_smoke $cargo_flag

echo "==> Building .app bundle at $bundle"
rm -rf "$bundle"
mkdir -p "$contents/MacOS" "$contents/Resources"
cp "$bin" "$contents/MacOS/$bundle_name"

cat > "$contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>${bundle_name}</string>
    <key>CFBundleIdentifier</key>
    <string>${bundle_id}</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>${bundle_name}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
    <key>CFBundleURLTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>CFBundleURLName</key>
            <string>${bundle_id}</string>
            <key>CFBundleURLSchemes</key>
            <array>
                <string>kael</string>
            </array>
        </dict>
    </array>
</dict>
</plist>
EOF

# Refresh Launch Services so the new (or rebuilt) bundle is registered as
# the kael:// handler before we launch the app. Without this the first
# kael://hi click can race the registration done at runtime.
lsregister="/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister"
if [[ -x "$lsregister" ]]; then
    "$lsregister" -f "$bundle" >/dev/null 2>&1 || true
fi

if [[ "${1:-}" == "--build" ]]; then
    echo "==> Bundle ready (skipping launch)."
    echo "    Open with: open \"$bundle\""
    exit 0
fi

echo "==> Launching $bundle"
echo "    Then open kael://hi in your browser — the URL should reach the app."
echo "    Logs from on_open_urls / eprintln! will stream here (via a tempfile)."
echo

# Launch via `open` so Launch Services routes kael:// URLs to this
# running instance — but `open` detaches stdio by default, which is why
# eprintln! never reached the terminal. We send the app's stderr to a
# tempfile and tail it in the foreground while the app runs, so every
# log line appears in this terminal while preserving Launch Services
# semantics for URL dispatch.
log_file="$(mktemp -t objc2-smoke-stderr.XXXXXX.log)"
echo "==> Streaming stderr from $log_file"
echo

# Tail in background and forward to our stderr; will exit when killed.
tail -n +1 -F "$log_file" &
tail_pid=$!

cleanup() {
    kill "$tail_pid" 2>/dev/null || true
    rm -f "$log_file"
}
trap cleanup EXIT INT TERM

open -W --stderr "$log_file" -a "$bundle"
