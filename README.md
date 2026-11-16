# Feint

A game-AI protocol and core whose bots win by thinking, not by being faster: they predict the
opponent, show one move and play another, and plan a long game, all under human-like limits on
reaction time and body speed. One core runs every game; each game is an adapter.

Six games run on it in the browser, each with five hand-made levels:

| Game | What the bot reads and fakes |
|---|---|
| Air hockey | the mallet's line before a shot; holds a show to one corner, shoots the other |
| Penalty shootout | the shooter's body before the kick; the keeper must guess |
| Quick-draw duel | the draw; a released draw is a fake, a step aside dodges the bullet |
| Table tennis | the paddle's facing before contact; a disguised shot shows the other corner |
| Boxing | the visible wind-up; a hook is shown, cancelled once the guard follows, and a jab goes to the other height |
| «Верю — не верю» (Cheat) | a card bluffing game against a trained policy, with levels that search one to three moves ahead |

There is also a trained (PPO) air-hockey bot, a local arena that stores every match and
re-verifies it by replaying it natively, and a C library with a Godot example to embed the core
in another engine.

## How it works

- **Protocol** (`schema/feint.fbs`, `docs/protocol-v1.md`): FlatBuffers messages for a game's
  spec, observations, actions, events and match records. It is game-neutral; nothing in it names
  a game. A match is a seed, the bodies and the actions, so anyone can replay it.
- **Adapter** (`crates/feint-protocol/src/adapter.rs`): what a game implements: reset, apply an
  action, advance a tick, observe for a seat, rebuild a state from an observation, simulate.
- **Core** (`crates/feint-ai`): the bot. It sees observations only, never the live state, with a
  reaction delay; it imagines each of its options against a model of the opponent, keeps a model
  of the player's habits (what they shoot, what they cover after a show), and decides when a fake
  is worth it. A difficulty preset sets reaction time, body, accuracy, search depth, deception and
  memory per level.
- **Determinism**: physics uses only + − × ÷ and sqrt on floats (`clippy.toml` bans the rest), so
  a match plays the same natively and in WebAssembly, and a record verifies byte for byte.

## Run it

Requirements: Rust (the pinned toolchain in `rust-toolchain.toml`, with the `wasm32-unknown-unknown`
target), the `wasm-bindgen` CLI at the version in `Cargo.lock` (0.2.128), and Node 20+.

```
scripts/web-build.sh
cargo run --locked --release -p feint-arena -- --dist web/dist --data data
```

Then open http://127.0.0.1:8787. Matches are stored under `data/` (ignored by git). Press D in a
match to see what the bot is thinking.

Checks:

```
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/wasm-check.sh
```

`scripts/` has the rest: `arena-check.sh` (a real arena, matches played in wasm and verified
natively), `ffi-check.sh` (the C header and example), `godot-check.sh` (needs Godot 4.7),
`gen-protocol.sh --check` (needs the official flatc 25.12.19). Training lives in `python/` (uv, PyTorch).

## Layout

| Path | What |
|---|---|
| `crates/feint-protocol` | generated protocol types, the adapter trait, encode and decode |
| `crates/feint-ai` | the core: bot, playbooks, opponent model, long game, search (ISMCTS), CFR |
| `crates/games/*` | one crate per game: rules, adapter, playbook, a scripted human model |
| `crates/feint-web` | the WebAssembly hub the browser plays through |
| `crates/feint-arena` | the local server: uploads, verification, SQLite and Parquet storage, profiles |
| `crates/feint-ffi`, `include/feint.h` | the C library; `examples/c`, `examples/godot` |
| `crates/feint-ml`, `crates/feint-turn`, `python/` | trained policies: training, export, inference in Rust |
| `crates/feint-compare` | the cross-game comparison shown on the hub |
| `web/` | the front (TypeScript, Vite) |
| `docs/` | the protocol, notes on connecting a physical table |

`plugins/`, `bench/` and `tests/` belong to the project template the repository started from;
they are not Feint code.

## Limits

- Level strength is measured against scripted human models, not a crowd of real players.
- The trained air-hockey bot does not fake: in air hockey a fake barely pays against these
  opponents, and training found plain shots better.
- Card results are practice: the browser knows the deal's seed.

## License

Apache License 2.0, see `LICENSE` and `NOTICE`. The vendored Godot header and the project
template under `plugins/` keep their MIT licences, listed in `NOTICE`.
