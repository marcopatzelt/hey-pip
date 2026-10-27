#!/usr/bin/env bash
# Screenshot of one preview scene with headless Firefox (own profile: no window, does not touch any other Firefox).
# Usage: tools/shot.sh <scene> <out.png> [extra query, e.g. "reduce=1"]  (PORT=... to choose the port)
# Profile in $SCRATCH/ffprof (default: a temp folder). Snap Firefox cannot see /tmp: then the profile and the raw
# PNG live in ~/snap/firefox/common/pip-shot (Firefox's own folder, not the user's profile) and the PNG is copied out.
#        tools/shot.sh tip-story [out dir]  = the four "Good to know" pictures (letter, tip with its three buttons,
#        Why?, the drafted question) as <out dir>/tip-{letter,bubble,why,offer}.png (default: design/readme).
set -euo pipefail
scene="${1:?scene name, e.g. celebrate}"
if [ "$scene" = "tip-story" ]; then
  self="$(realpath "$0")"
  dir="$(realpath -m "${2:-$(dirname "$self")/../../../design/readme}")"
  for part in letter bubble why offer; do
    "$self" "tip-$part" "$dir/tip-$part.png" "${3:-}"
  done
  exit 0
fi
out="$(realpath -m "${2:?output png}")"
extra="${3:-}"
here="$(cd "$(dirname "$0")" && pwd)"
SCRATCH="${SCRATCH:-${TMPDIR:-/tmp}/pip-shot}"
# 127.0.0.1:8765, or a free port when something else already listens there
port="${PORT:-8765}"
if python3 -c "import socket; socket.create_connection(('127.0.0.1', $port), 0.2)" 2>/dev/null; then
  port="$(python3 -c "import socket; s = socket.socket(); s.bind(('127.0.0.1', 0)); print(s.getsockname()[1])")"
fi
work="$SCRATCH"
[ -x /snap/bin/firefox ] && work="$HOME/snap/firefox/common/pip-shot"
mkdir -p "$work/ffprof" "$(dirname "$out")"
python3 "$here/preview_server.py" "$port" &
server=$!
trap 'kill "$server" 2>/dev/null || true' EXIT
for _ in $(seq 50); do
  python3 -c "import socket; socket.create_connection(('127.0.0.1', $port), 0.2)" 2>/dev/null && break
  sleep 0.1
done
rm -f "$out" "$work/shot.png"
timeout 90 firefox --no-remote --headless --profile "$work/ffprof" --window-size=1600,900 \
  --screenshot "$work/shot.png" "http://127.0.0.1:$port/tools/preview.html?shot=1${extra:+&$extra}#$scene" >/dev/null 2>&1
mv "$work/shot.png" "$out"
test -s "$out" && echo "$out"
