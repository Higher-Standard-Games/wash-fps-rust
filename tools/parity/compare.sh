#!/bin/bash
# Browser against Rust, the same views side by side (RUST.md 8.3).
#
#   tools/parity/compare.sh <map> <demo.shagdemo> <out prefix> "<x,y,z,yaw,pitch>;..."
#
# Views are Quake 3 positions and angles in degrees (pitch: negative looks up).
# The browser half needs a SHAG probe server on :3100 serving the fps game
# with the shipped packages (see README.md); SKIP_BROWSER=1 reuses its shots.
# Writes <prefix>_b_<i>.png, <prefix>_r_<i>.png and <prefix>_cmp_<i>.png and
# prints the mean difference per channel. PLAYERS="<x,y,z,yaw,clip,frame>;..."
# stands player models in both halves. ITEMS=<seconds> shows the procedural
# pickups in both, posed that many seconds in. PROJECTILES="<id,x,y,z,yaw,pitch>;..."
# holds projectiles in both (their shader stages ITEMS seconds in).
# EXPLOSIONS="<id,x,y,z,age>;..." holds those projectiles' explosions in both,
# that many seconds after they went off against a floor.
# EFFECTS="tracer:<args>;muzzle:<args>;impact:<args>;rail:<args>;lightning:<args>"
# holds bullet streaks, muzzle flashes, impact sparks, rails and Voltaic
# beams in both, with washoi view's --tracer, --muzzle, --impact, --rail and
# --lightning arguments. LIGHTS=1 keeps the projectiles' and explosions'
# dynamic lights in both (they are off otherwise).
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
map=$1; demo=$2; prefix=$(realpath -m "$3"); views=$4
if [ -z "$SKIP_BROWSER" ]; then
  NODE_PATH=$(npm root -g) node "$HERE/views.cjs" "$demo" "${prefix}_b" "$views" 1280x720 '^(bsp|skybox)' "$PLAYERS" "$ITEMS" "$PROJECTILES" "$EXPLOSIONS" "$EFFECTS" >/dev/null
fi
player_args=()
[ -z "$LIGHTS" ] && player_args+=(--no-lights)
[ -n "$ITEMS" ] && player_args+=(--time "$ITEMS")
if [ -n "$PROJECTILES" ]; then
  IFS=';' read -ra prlist <<< "$PROJECTILES"
  for p in "${prlist[@]}"; do player_args+=(--projectile "$p"); done
fi
if [ -n "$EXPLOSIONS" ]; then
  IFS=';' read -ra exlist <<< "$EXPLOSIONS"
  for p in "${exlist[@]}"; do player_args+=(--explosion "$p"); done
fi
if [ -n "$EFFECTS" ]; then
  IFS=';' read -ra eflist <<< "$EFFECTS"
  for p in "${eflist[@]}"; do player_args+=("--${p%%:*}" "${p#*:}"); done
fi
if [ -n "$PLAYERS" ]; then
  IFS=';' read -ra plist <<< "$PLAYERS"
  for p in "${plist[@]}"; do player_args+=(--player " $p"); done
fi
i=0
IFS=';' read -ra list <<< "$views"
for v in "${list[@]}"; do
  IFS=',' read -r x y z yaw pitch <<< "$v"
  (cd "$ROOT" && ./target/release/washoi view "$map" --at " $x,$y,$z,$yaw" --pitch " $pitch" "${player_args[@]}" --screenshot "${prefix}_r_$i.png" >/dev/null)
  python3 - "${prefix}" "$i" <<'PY'
import sys
from PIL import Image, ImageChops, ImageStat
p, i = sys.argv[1], sys.argv[2]
a = Image.open(f"{p}_b_{i}.png").convert("RGB")
b = Image.open(f"{p}_r_{i}.png").convert("RGB")
o = Image.new("RGB", (a.width, a.height * 2))
o.paste(a, (0, 0))
o.paste(b, (0, a.height))
o.resize((a.width * 3 // 4, a.height * 3 // 2)).save(f"{p}_cmp_{i}.png")
d = ImageStat.Stat(ImageChops.difference(a, b)).mean
print(f"view {i}: mean difference {[round(x, 1) for x in d]} (browser {[round(x) for x in ImageStat.Stat(a).mean]}, rust {[round(x) for x in ImageStat.Stat(b).mean]})")
PY
  i=$((i + 1))
done
