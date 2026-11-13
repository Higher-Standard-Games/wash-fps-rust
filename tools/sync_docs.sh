#!/bin/sh
# wash-fps is the master copy of the documents the lobby shows (TERMS.md,
# CREDITS.md); assets/docs/ holds copies, built into the client.
#
#   tools/sync_docs.sh [WASH_FPS_DIR]          copy them over
#   tools/sync_docs.sh --check [WASH_FPS_DIR]  fail when a copy is outdated
#
# WASH_FPS_DIR defaults to ../wash-fps. CI runs --check against wash-fps main.
set -eu

DOCS="TERMS.md CREDITS.md"
check=false
if [ "${1:-}" = "--check" ]; then
  check=true
  shift
fi
src=${1:-../wash-fps}
here=$(cd "$(dirname "$0")/.." && pwd)

status=0
for doc in $DOCS; do
  from="$src/$doc"
  to="$here/assets/docs/$doc"
  if [ ! -f "$from" ]; then
    echo "error: $from not found (pass the wash-fps checkout as an argument)" >&2
    exit 2
  fi
  if cmp -s "$from" "$to"; then
    continue
  fi
  if $check; then
    echo "::error file=assets/docs/$doc::assets/docs/$doc is outdated: wash-fps's $doc has changed. Run tools/sync_docs.sh to copy it over."
    diff -u "$to" "$from" || true
    status=1
  else
    cp "$from" "$to"
    echo "copied $doc"
  fi
done
exit $status
