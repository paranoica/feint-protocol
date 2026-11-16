import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { games, initSync, WebMatch } from "../src/wasm/feint_web.js";
import { assistValues, autoDepth, contactAim, lateralAt, makeCamera, project, readPaddles, shotValues } from "../src/games/pingpong.ts";
import { attackFromKey, boxingValues, guardFromKey, readFighters } from "../src/games/boxing.ts";

initSync({ module: readFileSync(new URL("../src/wasm/feint_web_bg.wasm", import.meta.url)) });

test("the built module plays a match to seven under node", () => {
  const game = new WebMatch(1, 7);
  assert.equal(game.tick_hz(), 240);
  let goals = 0;
  for (let i = 0; i < 240 * 60 * 20 && !game.finished(); i += 8) {
    const p = game.positions();
    assert.equal(p.length, 12);
    game.set_pointer(p[0], p[1]);
    goals += game.advance(8);
  }
  assert.ok(game.finished(), "no one reached 7");
  const score = Array.from(game.score());
  assert.ok(score.includes(7), `score ${score}`);
  assert.equal(goals, score[0] + score[1], "advance reports every goal");
  assert.ok(game.record().length > 100);
  assert.match(game.digest_hex(), /^[0-9a-f]{16}$/);
  game.free();
});

test("the debug readings have their shapes", () => {
  const easy = new WebMatch(1, 1);
  easy.advance(240);
  assert.equal(easy.memory(), false, "level 1 keeps no model");
  assert.deepEqual(Array.from(easy.likely_cover()), []);
  const hard = new WebMatch(5, 1);
  hard.advance(240);
  assert.equal(hard.memory(), true);
  assert.equal(hard.likely_cover().length, 3);
  assert.ok(hard.intent() >= 2, `intent ${hard.intent()}`);
  assert.equal(hard.likely_shot(), -1, "no shots seen yet");
  assert.deepEqual(Array.from(hard.class_point(0, 2)).map((v) => Math.round(v * 100) / 100), [1, 0.08]);
  assert.deepEqual(Array.from(hard.table()).map((v) => Math.round(v * 1000) / 1000), [1, 0.5, 0.15, 0.05, 0.07]);
  assert.ok(hard.current_real() >= -1 && hard.current_real() <= 2);
  assert.ok(hard.current_shown() >= -1 && hard.current_shown() <= 2);
  assert.throws(() => new WebMatch(0, 1));
  assert.throws(() => new WebMatch(6, 1));
});

test("a level-5 profile goes out and back in", () => {
  const first = new WebMatch(5, 3);
  for (let i = 0; i < 400; i++) {
    const p = first.positions();
    first.set_pointer(p[0], p[1]);
    first.advance(8);
  }
  const bytes = first.profile();
  assert.ok(bytes.length > 0);
  const second = WebMatch.new_with_profile(5, 4, bytes);
  assert.ok(second.profile().length > 0);
  assert.throws(() => WebMatch.new_with_profile(4, 4, bytes));
  assert.throws(() => WebMatch.new_with_profile(5, 4, new Uint8Array([1, 2, 3])));
  assert.equal(new WebMatch(2, 1).profile().length, 0);
});

test("the serve pause is readable and starts at a goal", () => {
  const game = new WebMatch(1, 7);
  assert.equal(game.serve_pause(), 0);
  let goals = 0;
  for (let i = 0; i < 240 * 300 && goals === 0; i++) {
    const p = game.positions();
    game.set_pointer(p[0], p[1]);
    goals = game.advance(1);
  }
  assert.equal(goals, 1);
  assert.ok(Math.abs(game.serve_pause() - 1) < 1e-6, `${game.serve_pause()}`);
  game.free();
});

test("two bots play for a watcher and each shows its own mind", () => {
  const game = WebMatch.spectate(1, 5, 0);
  assert.equal(game.spectating(), true);
  assert.equal(new WebMatch(1, 0).spectating(), false);
  assert.throws(() => WebMatch.spectate(0, 5, 0));
  let fakes = [0, 0];
  for (let i = 0; i < 240 * 600 && !game.finished(); i += 4) {
    game.advance(4);
    for (const seat of [0, 1]) {
      const intent = game.intent_of(seat);
      if (intent >= 0 && (intent & 1) === 1) {
        fakes[seat] += 1;
      }
    }
  }
  assert.ok(game.finished());
  assert.equal(fakes[0], 0, "level 1 never feints");
  assert.ok(fakes[1] > 0, "level 5 feints");
  assert.equal(game.memory_of(0), false);
  assert.equal(game.memory_of(1), true);
  assert.equal(game.likely_cover_of(1).length, 3);
  assert.ok(game.shown_of(0) >= -1 && game.real_of(1) <= 2);
  assert.equal(game.likely_shot_of(0), -1);
  game.free();
});

test("the hub builds a game by id and keeps the hockey entry points", () => {
  assert.deepEqual(Array.from(games()), ["airhockey", "penalty", "duel", "pingpong", "boxing"]);
  const game = WebMatch.create("airhockey", 2, 5);
  assert.equal(game.game_id(), "airhockey");
  assert.equal(game.arena(), true);
  assert.deepEqual(Array.from(game.layout()), Array.from(game.table()));
  game.set_input(new Float32Array([-0.5, 0.2]));
  game.advance(8);
  assert.equal(game.view().length, 12);
  const same = new WebMatch(2, 5);
  same.set_pointer(-0.5, 0.2);
  same.advance(8);
  assert.equal(game.digest_hex(), same.digest_hex(), "create and new play the same hockey match");
  assert.throws(() => WebMatch.create("chess", 2, 5));
  assert.throws(() => WebMatch.watch("chess", 1, 2, 5));
  assert.equal(WebMatch.watch("airhockey", 1, 5, 0).spectating(), true);
  game.free();
  same.free();
});

test("a penalty shootout runs in wasm with presses as one-shot inputs", () => {
  const game = WebMatch.create("penalty", 2, 3);
  assert.equal(game.arena(), true, "penalty shootouts are uploaded to the arena");
  assert.equal(game.layout().length, 5);
  let kicked = 0;
  for (let i = 0; i < 240 * 600 && !game.finished(); i += 4) {
    const v = game.view();
    if (v[0] === 0 && v[1] === 0 && v[2] < 0.6 && kicked < 10) {
      game.push_input(new Float32Array([0.8, 1, 1, 2.8]));
      kicked += 1;
    } else {
      game.set_input(new Float32Array([0, 1, 0, 0]));
    }
    game.advance(4);
  }
  assert.ok(game.finished(), "the shootout finished");
  assert.ok(Array.from(game.score()).some((g) => g > 0));
  game.free();
});

test("every hub view agrees with its wasm game on uploading to the arena", async () => {
  const host = globalThis as Record<string, unknown>;
  host.document ??= { documentElement: {} };
  host.getComputedStyle ??= () => ({ getPropertyValue: () => "" });
  const { GAMES } = await import("../src/games/index.ts");
  assert.deepEqual(GAMES.map((g) => g.id).sort(), Array.from(games()).sort());
  for (const view of GAMES) {
    const game = WebMatch.create(view.id, 1, 1);
    assert.equal(view.arena, game.arena(), view.id);
    game.free();
  }
});

test("a duel runs in wasm with a held draw and a pushed sidestep", () => {
  const game = WebMatch.create("duel", 2, 3);
  assert.equal(game.arena(), true);
  assert.equal(game.layout().length, 5);
  let stepped = 0;
  for (let i = 0; i < 240 * 900 && !game.finished(); i += 2) {
    const v = game.view();
    game.set_input(new Float32Array([v[7], 1, 0]));
    if (v[8] > 0.4 && v[6] === 0) {
      game.push_input(new Float32Array([v[7], 1, stepped % 2 === 0 ? -1 : 1]));
      stepped += 1;
    }
    game.advance(2);
  }
  assert.equal(game.finished(), true);
  const [a, b] = Array.from(game.score());
  assert.ok(Math.max(a, b) === 5, `${a}:${b}`);
  assert.ok(stepped > 0);
  game.free();
});

test("every hub game starts a level 5 match from that game's own profile", () => {
  for (const id of ["penalty", "duel", "pingpong", "boxing"]) {
    const first = WebMatch.create(id, 5, 1);
    for (let i = 0; i < 240 * 900 && !first.finished(); i += 16) {
      first.advance(16);
    }
    const profile = first.profile();
    assert.ok(profile.length > 0, id);
    const next = WebMatch.create_with_profile(id, 5, 2, profile);
    assert.equal(next.game_id(), id);
    assert.equal(next.memory_of(1), true);
    next.free();
    first.free();
    assert.throws(() => WebMatch.create_with_profile(id, 3, 2, profile), `${id}: level 3 takes no profile`);
  }
});

test("a table tennis match runs in wasm, the paddle follows the input and points are scored", () => {
  const game = WebMatch.create("pingpong", 3, 7);
  assert.equal(game.game_id(), "pingpong");
  assert.equal(game.arena(), true);
  assert.equal(game.layout().length, 6);
  const target: [number, number] = [-1.9, 0.3];
  game.set_input(new Float32Array(shotValues(target, 2, false, false)));
  game.advance(480);
  const v = Array.from(game.view());
  assert.equal(v.length, 14);
  const [mine] = readPaddles(v);
  assert.ok(Math.abs(mine.x - target[0]) < 0.05 && Math.abs(mine.y - target[1]) < 0.05, `${mine.x},${mine.y}`);
  for (let i = 0; i < 240 * 1800 && !game.finished(); i += 64) {
    game.advance(64);
  }
  assert.ok(game.finished());
  const [a, b] = Array.from(game.score());
  assert.ok(a + b >= 11, `${a}:${b}`);
  game.free();
});

test("boxing keys map to attacks and guards, and the values match the action space", () => {
  assert.deepEqual(attackFromKey("j"), [1, 1]);
  assert.deepEqual(attackFromKey("N"), [1, -1]);
  assert.deepEqual(attackFromKey("k"), [2, 1]);
  assert.deepEqual(attackFromKey("m"), [2, -1]);
  for (const reserved of ["d", "D", "f", "F", "x"]) {
    assert.equal(attackFromKey(reserved), null, `${reserved} belongs to the hub`);
    assert.equal(guardFromKey(reserved, 1), 1);
  }
  assert.equal(guardFromKey("s", 1), -1);
  assert.equal(guardFromKey("ArrowUp", -1), 1);
  assert.equal(guardFromKey("x", -1), -1);
  assert.deepEqual(boxingValues(-0.5, 1, null, false), [-0.5, 1, 0, 0, 0]);
  assert.deepEqual(boxingValues(-0.5, -1, [2, 1], false), [-0.5, -1, 2, 1, 0]);
  assert.deepEqual(boxingValues(-0.5, 1, null, true), [-0.5, 1, 0, 0, 1]);
});

test("a boxing match runs in wasm: the fighter walks to the input and a jab lands on the open height", () => {
  const game = WebMatch.create("boxing", 1, 7);
  assert.equal(game.game_id(), "boxing");
  assert.equal(game.arena(), true);
  assert.equal(game.layout().length, 5);
  game.set_input(new Float32Array(boxingValues(0.05, 1, null, false)));
  game.advance(240);
  const v = Array.from(game.view());
  assert.equal(v.length, 16);
  const [mine, theirs] = readFighters(v);
  assert.ok(Math.abs(mine.x - 0.05) < 0.01, `${mine.x}`);
  assert.ok(theirs.x - mine.x < 0.8);
  const open = theirs.guard > 0 ? -1 : 1;
  game.push_input(new Float32Array(boxingValues(0.05, 1, [1, open], false)));
  game.advance(4);
  const [winding] = readFighters(Array.from(game.view()));
  assert.equal(winding.stage, 1, "the jab winds up");
  assert.equal(winding.height, open);
  game.advance(240);
  assert.equal(Array.from(game.score())[0], 1, "the jab to the open height scores");
  for (let i = 0; i < 240 * 1800 && !game.finished(); i += 64) {
    game.advance(64);
  }
  assert.ok(game.finished());
  game.free();
});

const TABLE = { halfLength: 1.37, halfWidth: 0.7625, reach: 0.32, backRoom: 2, sideRoom: 1 };

test("table tennis keeps the paddle at the ready line and steps behind a ball that bounced on its side", () => {
  const view = (phase: number, bx: number, hitter: number, bounces: number, server = 1) => {
    const v = new Array(14).fill(0);
    v[0] = phase;
    v[2] = bx;
    v[5] = -1.5;
    v[11] = hitter;
    v[12] = bounces;
    v[13] = server;
    return v;
  };
  const ready = -(TABLE.halfLength + 0.4);
  assert.equal(autoDepth(view(1, 0.5, 0, 0), TABLE), ready, "my own ball goes away");
  assert.equal(autoDepth(view(1, 0.5, 1, 0), TABLE), ready, "theirs has not bounced yet");
  assert.ok(Math.abs(autoDepth(view(1, -1.0, 1, 1), TABLE) - -1.15) < 1e-9, "behind the bounced ball");
  assert.equal(autoDepth(view(1, -3.3, 1, 1), TABLE), -(TABLE.halfLength + TABLE.backRoom), "never past the back wall");
  assert.equal(autoDepth(view(0, 0, -1, 0, 0), TABLE), -1.5, "the server stays where it serves from");
  assert.equal(autoDepth(view(2, 0, 0, 0), TABLE), ready);
});

test("table tennis aims by where the ball meets the paddle", () => {
  assert.equal(contactAim(0.1, 0.1, 0.32), 0);
  assert.ok(Math.abs(contactAim(0.26, 0.1, 0.32) - 0.275) < 1e-9, "half the reach to the left: half way to the corner");
  assert.equal(contactAim(5, 0, 0.32), 0.55, "the edge sends it to the corner");
  assert.equal(contactAim(-5, 0, 0.32), -0.55);
});

test("the table tennis camera maps the mouse back to the side it shows", () => {
  const cam = makeCamera(1280, 720, TABLE);
  for (const [x, y] of [[-1.6, 0.4], [-1.2, -0.7], [0.5, 0.2]] as const) {
    const [sx] = project(cam, x, y, 0);
    assert.ok(Math.abs(lateralAt(cam, sx, x) - y) < 1e-9, `${x},${y}`);
  }
  const near = project(cam, -TABLE.halfLength, TABLE.halfWidth, 0);
  const far = project(cam, TABLE.halfLength, TABLE.halfWidth, 0);
  assert.ok(near[1] > far[1], "my end is at the bottom");
  assert.ok(Math.abs(near[0] - 640) > Math.abs(far[0] - 640), "the far end is narrower");
  assert.ok(near[0] < 640, "positive y is to my left");
  for (const [sx, sy] of [near, far]) {
    assert.ok(sx >= 0 && sx <= 1280 && sy >= 0 && sy <= 720, `${sx},${sy} on screen`);
  }
  for (const x of [-(TABLE.halfLength + 0.5), TABLE.halfLength + 0.5]) {
    const [, sy] = project(cam, x, 0, 0.25);
    assert.ok(sy > 20 && sy < 700, `a paddle at the ready line ${x} is in view: ${sy}`);
  }
});

test("a table tennis player who only follows the ball sideways with the assist beats level 1", () => {
  const game = WebMatch.create("pingpong", 1, 11);
  let returns = 0;
  let last = -1;
  while (!game.finished() && game.tick() < 240 * 900) {
    const v = Array.from(game.view());
    const serving = v[0] === 0 && v[13] === 0;
    const lateral = v[0] === 1 && v[11] === 1 ? v[3] : 0;
    game.set_input(new Float32Array(assistValues(v, lateral, TABLE, false, false, serving && v[1] > 1)));
    if (v[11] === 0 && last !== 0) {
      returns += 1;
    }
    last = v[11];
    game.advance(2);
  }
  assert.ok(game.finished());
  const [mine, theirs] = Array.from(game.score());
  assert.ok(mine > theirs, `${mine}:${theirs}`);
  assert.ok(returns > 20, `${returns}`);
  game.free();
});

test("a table tennis serve goes by itself after a second and a half, a click serves at once", () => {
  const serve = (t: number) => {
    const v = new Array(14).fill(0);
    v[1] = t;
    v[5] = -1.5;
    v[13] = 0;
    return v;
  };
  assert.equal(assistValues(serve(0.2), 0, TABLE, false, false, false)[2], 0, "waiting for the click");
  assert.equal(assistValues(serve(0.2), 0, TABLE, false, false, true)[2], 1, "a click serves");
  assert.equal(assistValues(serve(1.49), 0, TABLE, false, false, false)[2], 0);
  assert.equal(assistValues(serve(1.5), 0, TABLE, false, false, false)[2], 1, "no click: it serves anyway");
  const theirs = serve(2.5);
  theirs[13] = 1;
  assert.equal(assistValues(theirs, 0, TABLE, false, false, false)[2], 1, "a receiver keeps the swing ready");
});

