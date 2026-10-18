#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

APP_NAME="GitDaddy"
BUNDLE_ID="com.gitdaddy.app"
APP_BUNDLE="$SCRIPT_DIR/${APP_NAME}.app"
CONTENTS_DIR="$APP_BUNDLE/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"

echo "1. Building ${APP_NAME} (Release)..."
swift build -c release

BIN_PATH="$(swift build -c release --show-bin-path)/${APP_NAME}"

echo "2. Creating ${APP_NAME}.app bundle structure..."
rm -rf "$APP_BUNDLE"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

cp "$BIN_PATH" "$MACOS_DIR/$APP_NAME"
chmod +x "$MACOS_DIR/$APP_NAME"

echo "3. Packaging macOS AppIcon.icns from Assets/AppIcon.png..."
ICONSET_DIR="$(mktemp -d)/AppIcon.iconset"
mkdir -p "$ICONSET_DIR"

SOURCE_ICON="$SCRIPT_DIR/Assets/AppIcon.png"
if [ -f "$SOURCE_ICON" ]; then
    cp "$SOURCE_ICON" "$RESOURCES_DIR/AppIcon.png"
    sips -z 16 16     "$SOURCE_ICON" --out "$ICONSET_DIR/icon_16x16.png"      >/dev/null
    sips -z 32 32     "$SOURCE_ICON" --out "$ICONSET_DIR/icon_16x16@2x.png"   >/dev/null
    sips -z 32 32     "$SOURCE_ICON" --out "$ICONSET_DIR/icon_32x32.png"      >/dev/null
    sips -z 64 64     "$SOURCE_ICON" --out "$ICONSET_DIR/icon_32x32@2x.png"   >/dev/null
    sips -z 128 128   "$SOURCE_ICON" --out "$ICONSET_DIR/icon_128x128.png"    >/dev/null
    sips -z 256 256   "$SOURCE_ICON" --out "$ICONSET_DIR/icon_128x128@2x.png" >/dev/null
    sips -z 256 256   "$SOURCE_ICON" --out "$ICONSET_DIR/icon_256x256.png"    >/dev/null
    sips -z 512 512   "$SOURCE_ICON" --out "$ICONSET_DIR/icon_256x256@2x.png" >/dev/null
    sips -z 512 512   "$SOURCE_ICON" --out "$ICONSET_DIR/icon_512x512.png"    >/dev/null
    sips -z 1024 1024 "$SOURCE_ICON" --out "$ICONSET_DIR/icon_512x512@2x.png" >/dev/null
    iconutil -c icns "$ICONSET_DIR" -o "$RESOURCES_DIR/AppIcon.icns" || true
fi
rm -rf "$(dirname "$ICONSET_DIR")"

echo "4. Writing Info.plist..."
cat > "$CONTENTS_DIR/Info.plist" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>${APP_NAME}</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>${BUNDLE_ID}</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleDisplayName</key>
    <string>${APP_NAME}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>14.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
</dict>
</plist>
PLIST

echo "5. Signing app bundle (ad-hoc)..."
xattr -cr "$APP_BUNDLE" 2>/dev/null || true
codesign --force --deep --sign - "$APP_BUNDLE"
touch "$APP_BUNDLE"

echo "Done! App bundle created at: $APP_BUNDLE"

if [[ "${1:-}" == "--install" ]]; then
    TARGET_DIR="/Applications"
    if [ ! -w "$TARGET_DIR" ]; then
        TARGET_DIR="$HOME/Applications"
        mkdir -p "$TARGET_DIR"
    fi
    echo "6. Installing ${APP_NAME}.app to ${TARGET_DIR}..."
    rm -rf "${TARGET_DIR}/${APP_NAME}.app"
    cp -R "$APP_BUNDLE" "${TARGET_DIR}/${APP_NAME}.app"
    xattr -cr "${TARGET_DIR}/${APP_NAME}.app" 2>/dev/null || true
    touch "${TARGET_DIR}/${APP_NAME}.app"
    echo "Installed to ${TARGET_DIR}/${APP_NAME}.app"
fi
