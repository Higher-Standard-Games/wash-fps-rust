# Visual parity harness (RUST.md 8.3)

Renders the same views with the browser client and with `washoi view`, and
puts them side by side.

The browser half drives SHAG's fps game headless in Playwright's Chromium
(SwiftShader WebGL2). It plays a demo to load the map, pauses it, holds the
camera at each view (FOV 0.8 rad, as the Rust camera has it), hides every mesh
but the world, the sky and the map's weapon pickups (`bsp*`, `skybox*`, or the
regex given as the fifth argument of `views.cjs`), and hides the HUD. Weapon
pickups the demo has taken are shown again, as `washoi view` draws them all.

Set up once, in a SHAG checkout:

```bash
node scripts/postinstall.mjs                  # public/wasm/HavokPhysics.wasm
ln target/washoi-maps/pk3/*.pk3 ../shag/public/data/   # the shipped packages
# demos go in public/data/demos
SHAG_APP=src/games/fps node .claude/skills/browser-probe/scripts/private-vite.mjs 3100 &
```

Then, from this repository, with a release build of `washoi`:

```bash
tools/parity/compare.sh washoi_dm2 washoi_dm2_Deathmatch_20261101_144308.shagdemo \
  /tmp/dm2 "-744,-526.8,266.8,90,0"
```

A run takes a few minutes: the software GL renders about one frame a second.
