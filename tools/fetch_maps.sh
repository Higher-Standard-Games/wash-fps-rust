#!/usr/bin/env bash
# Downloads the shipped maps from the published game data and extracts their
# BSPs into <dir> (default target/washoi-maps), for tests/movement_maps_golden.rs:
#
#   tools/fetch_maps.sh [dir] && WASHOI_MAPS=target/washoi-maps cargo test
#
# Follows the client's startup (RUST.md 20.2): the manifest names the index,
# the index lists each package's SHA-256, and every package is checked against
# it. Packages already downloaded with the right digest are kept. Set
# WASHOI_DATA to use another region (default https://eu.washoi.net/data).
set -euo pipefail
dir=${1:-target/washoi-maps}
data=${WASHOI_DATA:-https://eu.washoi.net/data}
mkdir -p "$dir/pk3"
manifest=$(curl -fsS "$data/pk3_manifest.json")
index_file=$(printf '%s' "$manifest" | python3 -c 'import json,sys; print(json.load(sys.stdin)["indexFile"])')
curl -fsS -o "$dir/index.json" "$data/$index_file"
python3 - "$dir/index.json" <<'PY' > "$dir/maps.txt"
import json, sys
index = json.load(open(sys.argv[1]))
digests = {p["pk3"]: p["sha256"] for p in index["packages"]}
for name, pk3 in sorted(index["maps"].items()):
    print(name, pk3, digests[pk3])
PY
while read -r name pk3 sha; do
  file="$dir/pk3/$pk3"
  if ! echo "$sha  $file" | sha256sum -c --status 2>/dev/null; then
    echo "downloading $pk3"
    curl -fsS -o "$file.part" "$data/$pk3?sha256=$sha"
    echo "$sha  $file.part" | sha256sum -c --status || { echo "$pk3: SHA-256 does not match the index" >&2; exit 1; }
    mv "$file.part" "$file"
  fi
  unzip -o -q -j "$file" "maps/$name.bsp" -d "$dir"
done < "$dir/maps.txt"
ls "$dir"/*.bsp
