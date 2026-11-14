# Feint protocol v1.0

The protocol between a game and the Feint core: what a game says about itself, what each
player may see, how actions travel, and how a match is recorded so that anyone can replay
and verify it. It is game-neutral: nothing in it names a particular game. Three real-time
games (air hockey, penalty, duel) and two turn-based card games with hidden information
(Kuhn poker, «Верю — не верю») run on it unchanged.

The source of truth is [`schema/feint.fbs`](../schema/feint.fbs) (FlatBuffers). The Rust
types are generated from it by flatc 25.12.19 and committed
(`crates/feint-protocol/src/generated/`); `bedrock cmd gen-check` fails if they drift. The
`GameAdapter` trait in [`crates/feint-protocol/src/adapter.rs`](../crates/feint-protocol/src/adapter.rs)
is the other half of the contract.

## 1. Envelope

Every message travels in one `Envelope`:

| field | meaning |
|---|---|
| `protocol_version: string` | exactly `"1.0"` |
| `message: Message` | one of `GameSpec`, `Observation`, `Action`, `Event`, `MatchHeader`, `MatchResult`, `Profile` |

The buffer carries the FlatBuffers file identifier `FNT0`. `encode` writes an envelope;
`decode` is the only entry point for untrusted bytes and refuses, in this order:

1. more than 1 MiB (`MAX_MESSAGE_BYTES`) → `TooLarge`;
2. fewer than 8 bytes or no `FNT0` identifier → `NotFeint`;
3. a buffer the FlatBuffers verifier rejects (depth 16, 4096 tables, apparent size 1 MiB) → `Invalid`;
4. a version other than `"1.0"`, or none → `Version` (keeping at most 16 characters of what was sent);
5. an envelope without a message → `Empty`.

Versions are matched exactly; there is no negotiation. A `"0.1"` envelope is refused. Data
stored under 0.1 is upgraded once by `bedrock cmd migrate-v1` (section 9).

## 2. Describing a game: `GameSpec`

| field | meaning |
|---|---|
| `game_id` | a stable name, e.g. `airhockey`, `believe` |
| `game_version` | bumped on any change to rules or physics; a record replays only on its own version |
| `num_players` | 2 for every current game |
| `time_mode` | `Realtime`: every player may act on every tick. `TurnBased`: only `Observation.to_act` may act |
| `tick_hz` | ticks per second of game time (hockey 240, card games 10) |
| `decision_hz` | how often a controller decides; equals `tick_hz` for turn-based games |
| `action_space` | `continuous: [Dim]` for real-valued actions, `discrete: uint` for the number of discrete actions (0 when none) |
| `observation` | one `Dim` per number in `Observation.state` |
| `body_params` | one `Dim` per number of a player's body (`BodyParams`) |
| `signals` | one `Dim` per number of what a player shows the opponent |
| `intent_labels` | names for `Action.intent`; index 0 means "no intent"; for logs only |

`Dim` is `name`, `min`, `max`, `unit`.

## 3. Time

The tick is time, for every game. `advance` moves the game one tick whether or not anyone
acted. In a turn-based game an action is accepted only at the current tick and only from
the player named in `to_act`; a tick where the player to act is still thinking is an idle
tick, recorded by its passing and replayed like any other. A 10-minute card match at 10 Hz
is 6 000 ticks.

## 4. What a player sees: `Observation`

`observe(state, player)` fills one player's view of one tick, in that player's own frame
(in air hockey player 1 sees the table rotated 180°).

| field | meaning |
|---|---|
| `tick` | the tick it describes |
| `player` | whose view it is |
| `to_act` | turn-based: the player to act, `-1` when the match is over; real-time: always `-1` |
| `state` | the numbers described by `GameSpec.observation` |
| `opponent_signals` | what the opponent showed, described by `GameSpec.signals` |
| `legal` | turn-based: 1 per legal discrete action for the player to act, else empty |
| `score` | the score, signed, one entry per player |

Hidden information is whatever a player may not know; it never enters that player's
observation and never enters `opponent_signals`:

- The view must not change when only hidden facts change. The card games test this over
  whole histories: two deals where the opponent's batches have the same claim and count but
  different real cards give player 0 the same observations, several rounds deep.
- `signals(state, action)` turns an action into what the opponent sees. In «Верю — не верю»
  a lay shows `[0, claim, count]` and a call `[1, -1, 0]`; the real cards never leave the
  hand they were laid from until a call turns them over.
- `Action.discrete` may name hidden content (the real cards of a lay), so the raw action
  never reaches the opponent while a match runs. Records do carry it: a record is read
  after the match.
- "Known" features are what a careful person derives from what they saw, never card
  identities.
- Known limit (FNT-D44): the browser runs the card match itself and receives the deal seed
  first, so a script can compute the bot's hand. Card results are practice until the server
  reveals the deal only after the match (commit–reveal).

The bot reads observations only. `live()` exists for the runner, replay and the digest;
bot code that reads it is a bug.

## 5. Acting: `Action`, `Event`

| `Action` field | meaning |
|---|---|
| `tick` | the tick it is for; an action for another tick is refused (`WrongTick`) |
| `player` | who acts |
| `values` | continuous actions, one per `action_space.continuous` |
| `discrete` | the discrete action index, `-1` when none |
| `intent` | the actor's own label of what it means to do (index into `intent_labels`) |
| `deceptive` | whether the actor means it as a feint |

`intent` and `deceptive` are the actor's private notes for logs and training; they never
reach the opponent.

An action applies before its tick's step and holds until replaced. In a real-time game the
values enter physics as the f32 they are recorded as, so live play and replay agree bit
for bit.

`Event` is `tick`, `kind` (`Score`, `RoundEnd`, `Pause`, `Resume`) and `player` (`-1` when
the event belongs to nobody).

## 6. The `GameAdapter` contract

| method | contract |
|---|---|
| `spec()` | the `GameSpec` |
| `reset(seed, bodies)` | a new match; everything random comes from `seed` (ChaCha8) |
| `live()` | the true state; for the runner, replay and digest only |
| `apply(action)` | accept or refuse one action at the current tick; a refused action leaves the state unchanged |
| `advance(events)` | one tick of time; events of that tick are appended |
| `observe(state, player, out)` | a player's view (section 4) |
| `state_from_observation(obs, rng)` | a plausible full state consistent with a view, for planning; hidden facts are sampled |
| `simulate(state, actions, ticks)` | the rules run forward on a copy |
| `signals(state, action, out)` | what the opponent sees of an action |
| `digest(state)` | FNV-1a over the exact bits of the state at one tick; equal digests mean equal states at that tick, not equal histories (a penalty or duel digest covers the final score and phase, not every kick) |
| `result()` | `Some(MatchResult)` once the match is over |

Determinism rules for adapters (FNT-D4): floats only through `+ − × ÷` and `sqrt`
(`clippy.toml` bans the rest), no iteration over hash maps, no clocks. Then native code,
WebAssembly and the C interface play the same match to the same digest.

## 7. Matches: `MatchHeader`, `MatchResult`, records

`MatchHeader`: `game_id`, `game_version`, `seed`, `level` (the bot's level; card games use
the fixed preset 5), `bodies` (one `BodyParams` per player), `bot` (`HandMade` or `Trained`)
and `checkpoint` (the trained policy's id, 0 for a hand-made bot). A hand-made header writes
neither of the last two, so its bytes are the same as before they existed.

`MatchResult`: `score`, `duration_ticks`, `level`, `seed`.

A record is a byte stream of frames, each a little-endian `u32` length followed by one
envelope: one `MatchHeader`, then every accepted `Action` in tick order, then one
`MatchResult`. At most 64 MiB. A match is a function of its header and its actions: replay
resets with the header's seed and bodies, applies each action before its tick, advances
`duration_ticks` times and compares the digest. The arena verifies a record by playing it
again, the bot included, and refuses it unless re-encoding gives the uploaded bytes exactly.

## 8. `Profile`

What a level-5 hand-made bot learned about one player in one game, carried from match to
match: `game_id`, `classes`, and the counts and weights of its opponent model (`bite`,
`shots`, `mine`, `theirs`, `bluffs` with their event counts, `fake_tries`, `fake_wins`,
`knowing`, `reading`). The fields belong to the core's opponent model
(`crates/feint-ai/src/model.rs`); a game that has no such model never sends one.

## 9. Versioning and migration

- The version string changes when the meaning of stored bytes changes. Adding a field with
  a default, as `bot` and `checkpoint` were added, keeps old bytes readable and does not by
  itself need a new version.
- A new version refuses the old one; stored data is upgraded, not read twice.
- `bedrock cmd migrate-v1` upgrades `data/` from 0.1 to 1.0 with the arena stopped. Without
  arguments it only checks and writes nothing; `bedrock cmd migrate-v1 -- --write` migrates.
  It first upgrades every record and profile in memory and proves each new envelope differs
  from the old one only in the version string. Then it checks every record against what the
  arena stored when it accepted the match (digest, and the row's game, level, seed, ticks,
  score, finished flag and checkpoint), in the strongest way the record still allows:
  1. verified: the whole match is played again, the bot included, with the profile it was
     played with, and re-encodes to the same bytes;
  2. replayed: only when the hand-made bot now plays the match otherwise (`Mismatch`; the
     bot changed without a game version bump): the recorded inputs of both players are
     replayed through the game, every column the stored Parquet file has (at least `tick`,
     `events` and the game's columns; files from before stage 2 lack `checkpoint`) must
     equal the replay value for value at every tick, and the stored digest and score must
     match. The row's `profile_used` is not proven for these records: the replay does not
     run the bot, so a wrong profile and a changed bot look the same; the migration never
     writes that column;
  3. byte-proven: only when the record's game version is older than the one built today;
     nothing can replay it, so the byte proof alone carries it.

  Any other outcome stops the run before anything is written. A lock file keeps a second
  run out. Then it copies `data/` to `data/backup-0.1/` (built in a temporary folder,
  checked against the bytes about to be replaced, then renamed; an existing backup is kept
  and must hold those same bytes), rewrites the profiles in one transaction and the records
  (temporary file, then rename) after checking that the match and profile counts, the
  highest ids and every old byte are what the check read, and checks everything again from
  disk. Items already at 1.0 are checked but not rewritten, so a second run changes nothing.
- Old browser tabs still run the 0.1 WebAssembly module: rebuild `web/dist` with the arena
  and reload the page.

## 10. Changes since 0.1

v0.1 was the first schema (2026-11-09). Everything that changed until 1.0, with the reason.

### Wire format

| change | when | why |
|---|---|---|
| `Profile` message (`game_id` … `fake_wins`) | stage 1 | a level-5 bot keeps what it learned about a player between matches; the arena stores it per player |
| `Profile.theirs`, `theirs_events` | stage 1 | the bot ties what the opponent shows to where it really goes |
| `Profile.bluffs` | stage 1 | a show that ends without a shot is an outcome of its own |
| `Profile.knowing`, `reading` | stage 1 | which theory of the opponent covered more often travels with the player |
| `BotKind` enum, `MatchHeader.bot`, `MatchHeader.checkpoint` | stage 2 | a record names the trained policy it was played against, so the arena can rebuild it; defaults keep hand-made headers byte-identical |
| comments moved out of the schema | stage 1 | the meaning lives in this document |
| `protocol_version` `"0.1"` → `"1.0"`; 0.1 refused | stage 3 | the release; stored data migrated once |

### Meaning, without a wire change

| change | why |
|---|---|
| The tick is time in turn-based games too; idle ticks pass while nobody acts | a human thinking makes idle time; replay walks ticks, so counting actions as ticks broke replay of human matches |
| `opponent_signals` and `signals` never carry hidden content; the raw `Action` does not reach the opponent during a match | in a card game the discrete action names the real cards |
| `to_act = -1` ends a turn-based match | training and the CFR module detect terminal states from the observation alone |
| `legal` is the action mask for discrete policies | the trained card bot samples only legal moves |
| Card-game headers carry level 5 and a trained checkpoint; a hand-made card record is refused | no hand-made card bot exists; the arena needs a fixed preset to rebuild the match |

The `GameAdapter` trait has not changed since 0.1.

### Core and training code

Measured against the spec's demand of "the same core and training code without rework":
what had to be added to train and serve games with hidden information.

| change | why |
|---|---|
| `feint-turn` crate: `TurnGame`, `TurnBatch`, `TurnPolicy`, `TurnPilot` | the stage-2 training path was hockey-shaped (two Gaussian outputs, hockey batch, real-time pilot); a game-neutral path for discrete turn-based games sits beside it |
| `feint-gym` `TurnBatch(game, seats)` | Python reaches turn-based games through the same extension |
| Python: masked categorical head, a collector that keeps a seat's transition open until it acts again, PPO log-probability and entropy per sample, sampled evaluation, `turn-v1:<game>:<features>:<actions>` export | alternating players crashed the stage-2 collector; summing over the action axis collapsed a categorical batch; a deterministic bot cannot bluff at a mixed frequency |
| `feint-ai` `cfr` module: external-sampling MCCFR over `simulate`, information sets keyed by the acting player's observation | PPO's last iterate does not land inside Kuhn's equilibrium family; the spec allows CFR as a core module |
| `feint-safetensors` crate split out of `feint-ml` | game-neutral code reads policies without depending on the hockey model |
| Masked softmax with `exp` built from `+ − × ÷` | the float ban holds in serving; the arena re-runs a sampling bot and needs it bit-identical in native and WebAssembly |
| Arena: card records verified by replaying the trained bot; no Parquet for card matches | the same verify-by-replay rule as hockey; card matches have no physics frames |

Hockey after the stage: the hockey training path and exports are unchanged (a golden test
pins the hash of a fixed-seed export), and the measured results are identical to before
the stage (`bar`, `ml-ladder`, `strength`, `wasm-check`).

### Tests changed on purpose by the release

- `crates/feint-protocol/tests/roundtrip.rs`: the version string is `"1.0"`; the pinned bytes
  of a hand-made header differ only in the version string; a `"0.1"` envelope is refused.
- `crates/feint-arena/tests/fixtures/hand-made-v3-scripted.rec` stays at 0.1 as the input of
  the migration tests; upgraded, it verifies with the digest pinned before
  (`hand_made_records_still_verify_once_migrated_to_1_0`). The digest is of the game state,
  not of envelope bytes, so the version string does not touch it.
