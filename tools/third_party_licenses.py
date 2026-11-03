#!/usr/bin/env python3
"""Writes every crate the game links, with its licence texts, to stdout:
the THIRD_PARTY_LICENSES.txt that tools/package.sh ships beside the game.

Reads `cargo metadata` for the host's normal dependencies (no dev or build
dependencies), then copies each package's LICENSE*, LICENCE*, COPYING* and
NOTICE* files; a package without one is listed with its licence expression.
"""
import json
import os
import subprocess
import sys

meta = json.loads(
    subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--locked"], text=True)
)
packages = {p["id"]: p for p in meta["packages"]}
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
root = meta["resolve"]["root"]
seen, todo = set(), [root]
while todo:
    node = nodes[todo.pop()]
    for dep in node["deps"]:
        # Normal dependencies only: dev and build ones are not in the game.
        if any(k["kind"] is None for k in dep["dep_kinds"]) and dep["pkg"] not in seen:
            seen.add(dep["pkg"])
            todo.append(dep["pkg"])

out = sys.stdout
out.write("Washoi links the following crates. Their licences follow.\n\n")
chosen = sorted((packages[i] for i in seen), key=lambda p: (p["name"], p["version"]))
for p in chosen:
    out.write(f"  {p['name']} {p['version']}  ({p.get('license') or 'see below'})\n")
for p in chosen:
    folder = os.path.dirname(p["manifest_path"])

    def licence_files(folder):
        return sorted(
            os.path.join(folder, f) for f in os.listdir(folder)
            if f.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE"))
            and os.path.isfile(os.path.join(folder, f))
        )

    texts = licence_files(folder)
    # A workspace member's texts sit at the workspace root (shagger's NOTICE
    # credits the Babylon.js code in it).
    up = folder
    for _ in range(2):
        if texts:
            break
        up = os.path.dirname(up)
        texts = licence_files(up)
    # The engine's own crates carry vendored code with its texts beside it
    # (shagger's src/babylon: Babylon.js's Apache licence and NOTICE).
    if not (p.get("source") or "").startswith("registry+"):
        for base, _, files in os.walk(os.path.join(folder, "src")):
            texts += sorted(
                os.path.join(base, f) for f in files
                if f.upper().startswith(("LICENSE", "LICENCE", "NOTICE"))
            )
    out.write("\n" + "=" * 78 + f"\n{p['name']} {p['version']}\n" + "=" * 78 + "\n")
    if not texts:
        out.write(f"Licence: {p.get('license')}\n")
    for t in texts:
        with open(t, encoding="utf-8", errors="replace") as f:
            out.write(f"\n--- {os.path.basename(t)} ---\n{f.read()}\n")
