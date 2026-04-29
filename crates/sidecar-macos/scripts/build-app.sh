#!/usr/bin/env bash
#
# Builds the macOS sidecar binary and assembles it into a .app bundle
# at target/{profile}/X11WebSidecar.app.
#
# Why a bundle: macOS TCC keys Screen Recording grants on the codesign
# identity. Ad-hoc signing a bare binary derives that identity from the
# binary's content hash, so every `cargo build` invalidates the grant
# and the user has to re-add the entry. A .app bundle with a stable
# `CFBundleIdentifier` becomes the codesign identity instead, and
# survives rebuilds intact. (See cua-driver's `App/CuaDriverApp.app`
# for the same workaround.)
#
# Usage:
#   scripts/build-app.sh           # debug build
#   PROFILE=release scripts/build-app.sh
#
# After first build, drag target/debug/X11WebSidecar.app into
# System Settings → Privacy & Security → Screen Recording, toggle on,
# and run via:
#
#   open target/debug/X11WebSidecar.app
#
# or directly:
#
#   ./target/debug/X11WebSidecar.app/Contents/MacOS/x11-web-sidecar-macos

set -euo pipefail

PROFILE=${PROFILE:-debug}
BUNDLE_ID="com.theknarf.x11-web.sidecar-macos"
BIN_NAME="x11-web-sidecar-macos"
APP_NAME="X11WebSidecar"

CARGO_FLAGS=()
if [[ "$PROFILE" == "release" ]]; then
    CARGO_FLAGS+=("--release")
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$REPO_ROOT"

cargo build -p "$BIN_NAME" "${CARGO_FLAGS[@]}"

TARGET="$REPO_ROOT/target/$PROFILE"
APP="$TARGET/$APP_NAME.app"
CONTENTS="$APP/Contents"
MACOS="$CONTENTS/MacOS"

mkdir -p "$MACOS"
cp -f "$TARGET/$BIN_NAME" "$MACOS/$BIN_NAME"

cat > "$CONTENTS/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>$BIN_NAME</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>
    <string>x11-web sidecar</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleVersion</key>
    <string>0.1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>NSScreenCaptureUsageDescription</key>
    <string>x11-web-sidecar streams selected windows to a remote browser session.</string>
    <key>NSAccessibilityUsageDescription</key>
    <string>x11-web-sidecar mirrors application menus and synthesizes input on behalf of the remote browser session.</string>
    <key>LSUIElement</key>
    <true/>
    <key>LSMinimumSystemVersion</key>
    <string>14.0</string>
</dict>
</plist>
EOF

# Sign the bundle. With CFBundleIdentifier set, the codesign
# identifier derives from the bundle ID, but the *designated
# requirement* (which TCC actually keys against) depends on the
# signing identity. Ad-hoc burns the binary's cdhash into the
# requirement → TCC grants reset on every rebuild. A stable
# identity (our self-signed cert in login keychain) satisfies any
# rebuild. Falls back to ad-hoc if the cert isn't there yet.
SIGN_CERT="x11-web-sidecar-dev"
if security find-certificate -c "$SIGN_CERT" >/dev/null 2>&1; then
    SIGN_IDENTITY="$SIGN_CERT"
else
    cat <<'EOF'

Note: signing identity 'x11-web-sidecar-dev' not found in keychain.
      Bundle will be ad-hoc signed, which means each rebuild forces
      you to re-grant Screen Recording in System Settings.
      Run scripts/setup-signing.sh once to create a stable identity.

EOF
    SIGN_IDENTITY="-"
fi
codesign --force --sign "$SIGN_IDENTITY" "$APP"

echo "Built $APP"
codesign -dv "$APP" 2>&1 | sed 's/^/  /'
