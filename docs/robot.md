# Connecting a physical table

Notes for whoever puts the Feint core on a real robot. Stage 7 of the spec was cancelled
(FNT-D49), so nothing here is built; this page says where a robot plugs in and what changes.
Air hockey is the example because a robot mallet over a real table is the obvious first rig.

## What stays the same

- The protocol (`docs/protocol-v1.md`) and `GameAdapter`
  (`crates/feint-protocol/src/adapter.rs`) need no change. The bot only ever sees
  `ObservationT` and sends `ActionT` (FNT-D3), so it cannot tell a camera from the simulator.
- The core, the playbooks and the difficulty presets run unchanged. The air-hockey playbook
  already plans against the simulator through `simulate`, which stays the model of the table.
- The FFI library (`crates/feint-ffi`, `include/feint.h`) is the way in from a controller
  written in C or C++, as the Godot example (`examples/godot`) shows.

## The adapter for a real table

A `RealTable` adapter wraps the hardware and implements `GameAdapter` with the air-hockey
spec (`crates/games/airhockey/src/adapter.rs`), so observations and actions keep the same
names and units:

| Protocol | Real table |
|---|---|
| observation: puck x, y, vx, vy | the camera's puck track, in table metres, velocity from the last frames |
| observation: my x, y, vx, vy, target | the robot's own encoders, not the camera |
| observation: opponent x, y, vx, vy | the camera's track of the human's mallet |
| action: target x, y | a position command to the gantry or arm controller |
| body params: max speed, max accel | measured limits of the robot, not chosen ones |
| `advance` | wait for the next camera frame and the next control tick |
| `simulate`, `state_from_observation` | unchanged: the bot still imagines with the simulator |

The table geometry in `crates/games/airhockey/src/params.rs` must be set to the real table
(length, width, goal mouth, mallet and puck radii), measured, not left at the defaults.

## Time and delay

- The simulator runs 240 ticks/s and decides 60 times/s. A camera at 120–240 fps and a motor
  loop at 240 Hz or faster keep those numbers; a slower camera means a lower `tick_hz` in the
  spec, not resampling inside the core.
- The real delay (camera exposure, tracking, network, motor response) plays the role the
  difficulty's `reaction_ms` plays in the simulator. Measure it end to end (a light flash in
  view, time to the motor's first move) and subtract it from the level's reaction time, so a
  level-3 robot reacts like a level-3 bot and not slower.
- Anticipation (`crates/feint-ai/src/anticipate.rs`) already rolls the bot's own last actions
  forward over its delay; with a real delay it should use the measured one.

## What does not carry over

- Records cannot be verified by replay. The arena re-runs a match from its seed and inputs and
  expects the same digest (FNT-D4, FNT-D6); a real puck is not deterministic. A real match
  must store the observations the bot saw at each decision, and verification can only re-run
  the bot on those observations and check it chose the same actions.
- The digest of a real state means nothing; keep it for simulated matches only.
- The physics bans on floats (FNT-D4) still hold in the core, but the camera and motor code
  outside it are free to use whatever they need.

## Safety

- The motor controller, not the bot, owns the limits: a hard speed and force cap, soft walls
  short of the table edge, and a stop when tracking is lost for more than a few frames.
- `clamp_to_space` keeps actions inside the declared space; it is not a safety system.

## Order of work, if it is ever picked up

1. Camera tracking alone, compared with a hand-measured puck path.
2. The robot mallet following a target from a script, measuring its real speed, acceleration
   and delay.
3. `RealTable` playing level 1 against a person, with observations logged.
4. The levels re-measured against people, since the human model was tuned on the simulator.
