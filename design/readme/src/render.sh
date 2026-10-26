#!/bin/sh
# Renders the README images from the HTML sources with headless Firefox.
# Usage: design/readme/src/render.sh   (run from anywhere)
set -e
SRC=$(cd "$(dirname "$0")" && pwd)
OUT=$(dirname "$SRC")
PROFILE=$(mktemp -d "$HOME/snap/firefox/common/pip-readme-ff.XXXXXX")  # snap Firefox cannot use hidden folders
shot() { # url out width height
  firefox --headless --no-remote --profile "$PROFILE" --window-size "$3,$4" --screenshot "$2" "$1" >/dev/null 2>&1
}
shot "file://$SRC/banner.html"       "$OUT/pip-banner.png"       1600 500
shot "file://$SRC/banner.html#dark"  "$OUT/pip-banner-dark.png"  1600 500
shot "file://$SRC/how.html"          "$OUT/how-it-works.png"      1600 560
shot "file://$SRC/how.html#dark"     "$OUT/how-it-works-dark.png" 1600 560
rm -rf "$PROFILE"
# smaller files, same look
for f in "$OUT"/*.png; do magick "$f" -strip -define png:compression-level=9 "$f"; done
ls -la "$OUT"
