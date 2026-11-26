#!/bin/sh
# Construit dist/Tidy.app (release) : app sans icône dans le Dock (LSUIElement),
# pilotée depuis la barre des menus. Repris de Snappy.
#
# Usage : sh tools/bundle_macos.sh
#
# Signature : ad hoc par défaut. Avec un certificat « Developer ID Application »
# installé dans le trousseau, la variable TIDY_SIGN_IDENTITY le choisit (exécution
# renforcée et horodatage, prêts pour la notarisation) :
#   TIDY_SIGN_IDENTITY="Developer ID Application: Nom (ÉQUIPE)" sh tools/bundle_macos.sh
# Voir docs/distribution.md (signature, notarisation, .dmg avec tools/dmg_macos.sh).
#
# Attention : les autorisations de macOS (Fichiers et dossiers…) sont liées à la
# signature de l'app. Avec la signature ad hoc, l'identité change à chaque
# recompilation : macOS peut les redemander après chaque build. Un certificat stable
# évite ces demandes.
set -eu

cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)

cargo build --release -p tidy

APP=dist/Tidy.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/tidy "$APP/Contents/MacOS/tidy"
cp assets/icon.icns "$APP/Contents/Resources/icon.icns"

# Textes des demandes d'accès de macOS (Fichiers et dossiers) : les boîtes montrent
# le Bureau, la recherche parcourt Documents et Téléchargements.
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
  <key>CFBundleDevelopmentRegion</key><string>fr</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>Licence MIT</string>
  <key>NSDesktopFolderUsageDescription</key><string>Tidy affiche les éléments de ton bureau dans ses boîtes. Rien n'est déplacé ni supprimé.</string>
  <key>NSDocumentsFolderUsageDescription</key><string>La recherche rapide de Tidy parcourt tes documents pour les retrouver.</string>
  <key>NSDownloadsFolderUsageDescription</key><string>La recherche rapide de Tidy parcourt tes téléchargements pour les retrouver.</string>
</dict>
</plist>
PLIST

IDENTITY=${TIDY_SIGN_IDENTITY:-}
if [ -n "$IDENTITY" ]; then
  # Exécution renforcée (exigée par la notarisation), horodatage d'Apple.
  codesign --force --options runtime --timestamp --sign "$IDENTITY" "$APP"
  codesign --verify --strict --verbose=2 "$APP"
else
  # Signature ad hoc : nécessaire pour que macOS accepte de lancer le bundle (voir la
  # remarque en tête sur les autorisations).
  codesign --force --sign - "$APP" >/dev/null 2>&1 || echo "codesign indisponible : app non signée"
fi

du -sh "$APP"
echo "OK : $APP"
