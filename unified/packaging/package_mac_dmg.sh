#!/usr/bin/env bash
set -euo pipefail

# Builds a Universal macOS .dmg (Apple Silicon aarch64 + Intel x86_64) and SHA-256 checksums
# from the unified Rust codebase.
#
# Usage:
#   ./unified/packaging/package_mac_dmg.sh               # Universal build (Apple Silicon + Intel)
#   ./unified/packaging/package_mac_dmg.sh --native-only # Fast local build for current Mac architecture only

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
UNIFIED_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$UNIFIED_DIR"

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

APP_NAME="GitDaddy"
VERSION=$(grep -m1 '^version *=' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')
DIST_DIR="$UNIFIED_DIR/dist"
APP_BUNDLE="$DIST_DIR/$APP_NAME.app"
CONTENTS_DIR="$APP_BUNDLE/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"

rm -rf "$DIST_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

if [[ "${1:-}" == "--native-only" ]]; then
    echo "==> Building native macOS release binary for $APP_NAME v$VERSION..."
    cargo build --release --locked
    cp "target/release/$APP_NAME" "$MACOS_DIR/$APP_NAME"
else
    echo "==> Ensuring Apple Silicon (aarch64-apple-darwin) and Intel (x86_64-apple-darwin) targets are installed..."
    rustup target add aarch64-apple-darwin x86_64-apple-darwin

    echo "==> Building $APP_NAME v$VERSION for Apple Silicon (aarch64-apple-darwin)..."
    cargo build --release --locked --target aarch64-apple-darwin

    echo "==> Building $APP_NAME v$VERSION for Intel Mac (x86_64-apple-darwin)..."
    cargo build --release --locked --target x86_64-apple-darwin

    echo "==> Combining Apple Silicon + Intel binaries into a Universal binary via lipo..."
    lipo -create -output "$MACOS_DIR/$APP_NAME" \
        "target/aarch64-apple-darwin/release/$APP_NAME" \
        "target/x86_64-apple-darwin/release/$APP_NAME"
fi

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

echo "==> Ad-hoc signing $APP_NAME.app..."
codesign --force --deep --sign - "$APP_BUNDLE"

DMG_NAME="GitDaddy-$VERSION-macOS-universal.dmg"
ZIP_NAME="GitDaddy-macOS.zip"
SUMS_NAME="GitDaddy-$VERSION-macOS-SHA256SUMS.txt"

STAGING_DIR=$(mktemp -d)
cp -R "$APP_BUNDLE" "$STAGING_DIR/$APP_NAME.app"
ln -s /Applications "$STAGING_DIR/Applications"

echo "==> Creating $DMG_NAME..."
hdiutil create \
    -volname "$APP_NAME" \
    -srcfolder "$STAGING_DIR" \
    -ov \
    -format UDZO \
    "$DIST_DIR/$DMG_NAME" >/dev/null

rm -rf "$STAGING_DIR"

echo "==> Creating $ZIP_NAME..."
ditto -c -k --sequesterRsrc --keepParent "$APP_BUNDLE" "$DIST_DIR/$ZIP_NAME"

# Remove intermediate .app folder so dist/ contains only the release files
rm -rf "$APP_BUNDLE"

echo "==> Writing $SUMS_NAME..."
(
    cd "$DIST_DIR"
    shasum -a 256 "$DMG_NAME" "$ZIP_NAME" > "$SUMS_NAME"
)

echo "==> Packaged macOS release files in $DIST_DIR:"
ls -lh "$DIST_DIR"
