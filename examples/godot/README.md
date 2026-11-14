# Feint inside Godot

A Godot 4.7 project that runs an air-hockey match through the Feint C interface (`include/feint.h`): the bot plays one side, you play the other with the mouse. The engine talks to Feint through a GDExtension written in plain C (`extension/feint_godot.c`), with no godot-cpp.

## Build and run

```sh
scripts/godot-check.sh          # builds bin/libfeint_godot.so and checks it
godot --path examples/godot     # play: move the mouse, your mallet follows
godot -e --path examples/godot  # open the project in the editor
```

`godot-check` builds the extension against `target/release/libfeint_ffi.a` (statically, so Godot needs no extra library path; only the entry point `feint_godot_init` is exported), imports the project, plays a level 1 against level 5 match headless with `--check`, and compares the result line with the native Rust match. Without Godot on the machine it says it skipped.

## The class

`FeintMatch` (a `RefCounted`):

| Method | What it does |
|---|---|
| `start(seed, level_a, level_b)` | a new match; `level_a` 0 means you play seat 0, 1–5 a bot; `level_b` 1–5 |
| `step(x, y)` | one tick at 240 Hz; `x, y` is your mallet target in table metres; 1 when the match is over |
| `position(i)` | puck x, y, vx, vy, then each mallet's x, y, vx, vy |
| `layout(i)` | half length, half width, goal mouth half width, puck radius, mallet radius |
| `score(seat)`, `finished()` | the score and whether the match ended |
| `play_out(cap)` | plays a bot-against-bot match to its end in one call and returns `score A:B after N ticks digest X` |

## Notes

- The library is declared for `editor_runtime` and export templates only, so the editor itself does not load it: with the class registered in the editor, Godot 4.7.2 crashes on closing, even for an abstract class that is never instantiated (narrowed down to registering the class; cause not found). The class is not offered in the editor's autocompletion; running the project from the editor works.
- Instances are built with `classdb_construct_object3`: the older `classdb_construct_object2` left every `FeintMatch` with one reference too many, so none was ever freed. `godot-check` fails if Godot reports a leaked object.
- `extension/gdextension_interface.h` was produced by `godot --headless --dump-gdextension-interface` from Godot 4.7.2 and is Godot's file under the MIT licence (`extension/GODOT-LICENSE.txt`).
