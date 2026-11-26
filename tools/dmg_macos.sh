#!/bin/sh
# Construit dist/Tidy-<version>.dmg : Tidy.app et un raccourci vers Applications, à
# glisser l'un sur l'autre.
#
# Usage : sh tools/dmg_macos.sh
#
# - TIDY_SKIP_BUNDLE=1 : reprend dist/Tidy.app tel quel (sinon tools/bundle_macos.sh
#   le reconstruit, avec TIDY_SIGN_IDENTITY s'il est donné) ;
# - TIDY_SIGN_IDENTITY : signe aussi le .dmg ;
# - TIDY_NOTARY_PROFILE : profil de `xcrun notarytool store-credentials` ; le .dmg est
#   alors envoyé à la notarisation d'Apple, puis le ticket est agrafé.
# Voir docs/distribution.md.
set -eu

cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)

if [ "${TIDY_SKIP_BUNDLE:-}" != "1" ]; then
  sh tools/bundle_macos.sh
fi
APP=dist/Tidy.app
[ -d "$APP" ] || { echo "dist/Tidy.app absent : lance d'abord tools/bundle_macos.sh"; exit 1; }

DMG="dist/Tidy-${VERSION}.dmg"
STAGE=$(mktemp -d "${TMPDIR:-/tmp}/tidy-dmg.XXXXXX")
trap 'rm -rf "$STAGE"' EXIT
cp -R "$APP" "$STAGE/Tidy.app"
ln -s /Applications "$STAGE/Applications"

rm -f "$DMG"
hdiutil create -quiet -volname "Tidy" -srcfolder "$STAGE" -fs HFS+ -format UDZO -ov "$DMG"

IDENTITY=${TIDY_SIGN_IDENTITY:-}
if [ -n "$IDENTITY" ]; then
  codesign --force --timestamp --sign "$IDENTITY" "$DMG"
fi
PROFILE=${TIDY_NOTARY_PROFILE:-}
if [ -n "$PROFILE" ]; then
  xcrun notarytool submit "$DMG" --keychain-profile "$PROFILE" --wait
  xcrun stapler staple "$DMG"
fi

du -h "$DMG"
echo "OK : $DMG"
