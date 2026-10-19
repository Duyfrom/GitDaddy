#!/usr/bin/env bash
set -euo pipefail

# One-command macOS launcher for the unified GitDaddy codebase.
# Usage:
#   ./unified/run_mac.sh            # Builds and launches GitDaddy on your Mac
#   ./unified/run_mac.sh --install  # Also installs GitDaddy.app into /Applications
#   ./unified/run_mac.sh --no-open  # Builds GitDaddy.app without opening the window

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

VERSION=$(grep -m1 '^version *=' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')
APP_NAME="GitDaddy"
DIST_DIR="$SCRIPT_DIR/dist"
APP_BUNDLE="$DIST_DIR/$APP_NAME.app"
CONTENTS_DIR="$APP_BUNDLE/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"

echo "==> Building unified $APP_NAME v$VERSION for macOS..."
cargo build --release

rm -rf "$APP_BUNDLE"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"
cp "target/release/$APP_NAME" "$MACOS_DIR/$APP_NAME"
chmod +x "$MACOS_DIR/$APP_NAME"

if [ -f "assets/AppIcon.png" ]; then
    cp "assets/AppIcon.png" "$RESOURCES_DIR/AppIcon.png"
    ICONSET_DIR=$(mktemp -d)/AppIcon.iconset
    mkdir -p "$ICONSET_DIR"
    sips -z 16 16     "assets/AppIcon.png" --out "$ICONSET_DIR/icon_16x16.png"      >/dev/null 2>&1
    sips -z 32 32     "assets/AppIcon.png" --out "$ICONSET_DIR/icon_16x16@2x.png"   >/dev/null 2>&1
    sips -z 32 32     "assets/AppIcon.png" --out "$ICONSET_DIR/icon_32x32.png"      >/dev/null 2>&1
    sips -z 64 64     "assets/AppIcon.png" --out "$ICONSET_DIR/icon_32x32@2x.png"   >/dev/null 2>&1
    sips -z 128 128   "assets/AppIcon.png" --out "$ICONSET_DIR/icon_128x128.png"    >/dev/null 2>&1
    sips -z 256 256   "assets/AppIcon.png" --out "$ICONSET_DIR/icon_128x128@2x.png" >/dev/null 2>&1
    sips -z 256 256   "assets/AppIcon.png" --out "$ICONSET_DIR/icon_256x256.png"    >/dev/null 2>&1
    sips -z 512 512   "assets/AppIcon.png" --out "$ICONSET_DIR/icon_256x256@2x.png" >/dev/null 2>&1
    sips -z 512 512   "assets/AppIcon.png" --out "$ICONSET_DIR/icon_512x512.png"    >/dev/null 2>&1
    sips -z 1024 1024 "assets/AppIcon.png" --out "$ICONSET_DIR/icon_512x512@2x.png" >/dev/null 2>&1
    iconutil -c icns "$ICONSET_DIR" -o "$RESOURCES_DIR/AppIcon.icns" >/dev/null 2>&1 || true
    rm -rf "$(dirname "$ICONSET_DIR")"
fi

cat > "$CONTENTS_DIR/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>$APP_NAME</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>com.gitdaddy.app</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>
    <string>GitDaddy</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF

codesign --force --deep --sign - "$APP_BUNDLE" >/dev/null 2>&1 || true

if [[ "${1:-}" == "--install" ]]; then
    echo "==> Installing $APP_NAME.app into /Applications..."
    rm -rf "/Applications/$APP_NAME.app"
    cp -R "$APP_BUNDLE" "/Applications/$APP_NAME.app"
    echo "==> Installed to /Applications/$APP_NAME.app"
fi

if [[ "${1:-}" != "--no-open" ]]; then
    echo "==> Launching $APP_BUNDLE..."
    open "$APP_BUNDLE"
else
    echo "==> Built $APP_BUNDLE (skipped launch due to --no-open)."
fi
