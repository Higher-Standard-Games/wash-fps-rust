#!/usr/bin/env bash
# The webview lobby's smoke test (docs/research/WEBVIEW_LOBBY.md, W5): start
# the native client on a region's /client page and wait until the page has
# loaded and said hello over the bridge, then stop it. A screenshot of the
# window is kept where the desktop allows one.
#
#   tools/lobby_smoke/run.sh <washoi binary> <out dir> [seconds]
#
# WASHOI_LOBBY_PAGE picks another page (a local SHAG dev server). The client
# runs --offline on a stand-in package: the lobby page needs no game data.
set -u
exe=$1
out=$2
limit=${3:-180}
mkdir -p "$out/data"
out=$(cd "$out" && pwd)
# `--offline` wants a package on disk; the lobby never opens it.
: > "$out/data/stand-in.pk3"
log=$out/client.log

"$exe" --region eu --offline --data "$out/data" > "$log" 2>&1 &
pid=$!

shot() {
  case "$(uname -s)" in
    Linux) import -window root "$out/$1.png" 2>/dev/null ;;
    Darwin) screencapture -x "$out/$1.png" 2>/dev/null ;;
    MINGW* | MSYS* | CYGWIN*)
      powershell -NoProfile -Command "Add-Type -AssemblyName System.Windows.Forms,System.Drawing;
        \$b=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds;
        \$i=New-Object System.Drawing.Bitmap \$b.Width,\$b.Height;
        [System.Drawing.Graphics]::FromImage(\$i).CopyFromScreen(\$b.Location,[System.Drawing.Point]::Empty,\$b.Size);
        \$i.Save('$(cygpath -w "$out/$1.png" 2>/dev/null || echo "$out/$1.png")')" 2>/dev/null ;;
  esac
  true
}

status=1
why="no hello from the page in ${limit}s"
for _ in $(seq "$limit"); do
  if grep -q "lobby: the page said hello" "$log"; then
    status=0
    break
  fi
  if ! kill -0 "$pid" 2>/dev/null; then
    why="the client stopped before the page said hello"
    break
  fi
  sleep 1
done
# Let the page draw before the picture.
[ $status = 0 ] && sleep 8
shot lobby
kill "$pid" 2>/dev/null
wait "$pid" 2>/dev/null
cat "$log"
if [ $status = 0 ]; then
  echo "lobby smoke: ok"
else
  echo "lobby smoke: $why"
fi
exit $status
