#!/bin/sh
# Construit dist/Tidy.app (release) : app sans icône dans le Dock (LSUIElement),
# pilotée depuis la barre des menus. Repris de Snappy.
#
# Usage : sh tools/bundle_macos.sh
#
# Attention : les autorisations de macOS (Accessibilité, Calendriers…) sont liées à la
# signature de l'app. Avec la signature ad hoc ci-dessous, l'identité change à chaque
# recompilation : macOS peut les redemander après chaque build. Un certificat de
# développeur stable évitera ces demandes (vague 3 : signature et notarisation).
set -eu

cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)

cargo build --release -p tidy

APP=dist/Tidy.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/tidy "$APP/Contents/MacOS/tidy"
cp assets/icon.icns "$APP/Contents/Resources/icon.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Tidy</string>
  <key>CFBundleDisplayName</key><string>Tidy</string>
  <key>CFBundleIdentifier</key><string>com.tidy.app</string>
  <key>CFBundleExecutable</key><string>tidy</string>
  <key>CFBundleIconFile</key><string>icon.icns</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Signature ad hoc : nécessaire pour que macOS accepte de lancer le bundle (voir la
# remarque en tête sur les autorisations).
codesign --force --sign - "$APP" >/dev/null 2>&1 || echo "codesign indisponible : app non signée"

du -sh "$APP"
echo "OK : $APP"
