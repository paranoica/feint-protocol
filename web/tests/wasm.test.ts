import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { initSync, WebMatch } from "../src/wasm/feint_web.js";

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
