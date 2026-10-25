#!/bin/sh
# Builds dist/Doodoro GPUI.app: the release binary, the tomato icon and the fonts' licences (the
# fonts themselves are compiled into the binary). Ad-hoc signed, for this Mac.
set -e
cd "$(dirname "$0")"

cargo build --release

APP="dist/Doodoro GPUI.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/doodoro "$APP/Contents/MacOS/doodoro"
cp icon.icns "$APP/Contents/Resources/icon.icns"
cp fonts/OFL-*.txt "$APP/Contents/Resources/"

# LSUIElement: the shim turns the app into an accessory anyway (no Dock tile while it runs, so it
# can float over full-screen apps); declaring it up front saves the tile flashing in and out.
cat > "$APP/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key><string>Doodoro GPUI</string>
	<key>CFBundleDisplayName</key><string>Doodoro GPUI</string>
	<key>CFBundleIdentifier</key><string>local.doodoro.gpui</string>
	<key>CFBundleExecutable</key><string>doodoro</string>
	<key>CFBundleIconFile</key><string>icon</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>CFBundleShortVersionString</key><string>1.0</string>
	<key>CFBundleVersion</key><string>1</string>
	<key>LSMinimumSystemVersion</key><string>12.0</string>
	<key>LSUIElement</key><true/>
	<key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF

codesign --force --deep --sign - "$APP"
echo "built $APP"
