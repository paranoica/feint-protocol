import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { games, initSync, WebMatch } from "../src/wasm/feint_web.js";

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
  assert.deepEqual(Array.from(games()), ["airhockey", "penalty", "duel"]);
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
  for (const id of ["penalty", "duel"]) {
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
