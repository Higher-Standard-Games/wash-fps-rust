# Instructions for agents

- Read `docs/research/RUST.md` before structural changes. It
  is the plan; update it when a decision changes.
- Port wash-fps TypeScript one to one where Rust allows. Keep wash-fps's folder and
  file names (snake_case), and name the TypeScript source at the top of each
  ported module.
- Simulation and protocol code must match V8 bit for bit: use
  `shagger::shared::js` (`js_round`, `js_hypot`) instead of `f64::round` or
  `f64::hypot` wherever the TypeScript calls `Math.round` or `Math.hypot`.
  Prove each port with golden vectors (shagger's `tools/golden/generate.mjs`).
- Code ported or copied from Babylon.js goes in shagger's `crates/shagger/src/babylon/`
  only, with an Apache-2.0 header and a `NOTICE` entry. Everything else is MIT.
- Engine code (anything not specific to Washoi) belongs in shagger, not here (RUST.md 14.1).
- Before pushing: `cargo fmt --all --check`, `cargo clippy
  --all-targets -- -D warnings`, `cargo test`.
