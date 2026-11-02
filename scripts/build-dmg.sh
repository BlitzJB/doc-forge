#!/bin/sh
# Build Doc Forge and package it as a disk image for the landing page's download button:
#   site/downloads/Doc-Forge.dmg  (the app plus an Applications shortcut to drag it onto)
# Uses hdiutil directly, so no Finder scripting or extra permissions are needed.
set -eu
cd "$(dirname "$0")/.."

npx tauri build --bundles app
app="src-tauri/target/release/bundle/macos/Doc Forge.app"
[ -d "$app" ] || { echo "build-dmg: $app is missing" >&2; exit 1; }

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"

mkdir -p site/downloads
out=site/downloads/Doc-Forge.dmg
rm -f "$out"
hdiutil create -volname "Doc Forge" -srcfolder "$stage" -fs HFS+ -format UDZO -imagekey zlib-level=9 -ov "$out" >/dev/null
hdiutil verify "$out" >/dev/null
echo "$out ($(du -h "$out" | cut -f1))"
