#!/bin/sh
# The bundle the dot runs as. `swift build` alone is not enough: without the
# Info.plist beside it the app opens a window instead of living in the menu
# bar, and without the signature macOS refuses to start it at all.
#
# Usage: menubar/make-the-app.sh [destination]   (default: ~/.config/sailor/bin)
set -eu
here=$(cd "$(dirname "$0")" && pwd)
into=${1:-$HOME/.config/sailor/bin}
app=$into/SailorDot.app

cd "$here"
swift build -c release
mkdir -p "$app/Contents/MacOS"
cp "$here/Info.plist" "$app/Contents/Info.plist"
cp .build/release/SailorDot "$app/Contents/MacOS/SailorDot"
codesign --force --deep --sign - "$app"
echo "built $app — open it with: open '$app'"
