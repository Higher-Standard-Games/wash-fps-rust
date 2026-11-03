# wash-fps-rust

Washoi's native client: [wash-fps](https://github.com/Higher-Standard-Games/wash-fps)
ported to Rust, on the [SHAGGER](https://github.com/Higher-Standard-Games/shagger)
engine. It connects to the same servers as the browser client, and reads the same
published game data.

The plan, the findings behind it and the milestones are in
[docs/research/RUST.md](docs/research/RUST.md).

## Building

shagger is a private git dependency, pinned by revision in `Cargo.toml`.

```sh
CARGO_NET_GIT_FETCH_WITH_CLI=true cargo run
```

To work on the engine and the game together, clone shagger next to this
repository and add an untracked `.cargo/config.toml`:

```toml
[patch."https://github.com/Higher-Standard-Games/shagger"]
shagger = { path = "../shagger/crates/shagger" }
shagger_shared = { path = "../shagger/crates/shagger_shared" }
```

## Licenses

MIT. The engine's `src/babylon/` folder is Apache-2.0 (RUST.md 21.1); the
binary ships both license texts.

## Checks

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo deny check
```

shagger is public, so CI and release builds need no token to fetch it.
