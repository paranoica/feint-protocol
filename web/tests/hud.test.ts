import { test } from "node:test";
import assert from "node:assert/strict";
import { countdown, follow, goalBanner, shade } from "../src/hud.ts";

test("the goal banner names the scorer and fills as the pause runs out", () => {
  assert.equal(goalBanner(0, 1, 0), null, "no pause, no banner");
  assert.equal(goalBanner(0.5, 1, null), null, "a stall pause has no scorer");
  assert.deepEqual(goalBanner(1, 1, 0), { text: "Goal!", side: 0, progress: 0 });
  assert.deepEqual(goalBanner(0.25, 1, 1), { text: "Feint scores", side: 1, progress: 0.75 });
  assert.deepEqual(goalBanner(2, 1, 1), { text: "Feint scores", side: 1, progress: 0 }, "clamped");
});

test("the start countdown counts 3, 2, 1 and then gets out of the way", () => {
  assert.equal(countdown(1800, 600), "3");
  assert.equal(countdown(1201, 600), "3");
  assert.equal(countdown(1200, 600), "2");
  assert.equal(countdown(1, 600), "1");
  assert.equal(countdown(0, 600), null);
  assert.equal(countdown(-5, 600), null);
});

test("shading darkens a hex colour and leaves anything else alone", () => {
  assert.equal(shade("#22d3ee", 0.5), "rgb(17, 106, 119)");
  assert.equal(shade("#FFFFFF", 0), "rgb(0, 0, 0)");
  assert.equal(shade("#ffffff", 2), "rgb(255, 255, 255)", "clamped");
  assert.equal(shade("red", 0.5), "red");
});

test("the puck tail keeps the last points and drops them after a jump", () => {
  let tail: Array<[number, number]> = [];
  for (let i = 0; i < 5; i++) {
    tail = follow(tail, [i * 0.25, 0], 3, 0.3);
  }
  assert.deepEqual(tail, [[0.5, 0], [0.75, 0], [1, 0]]);
  assert.deepEqual(follow(tail, [0.2, 0], 3, 0.3), [[0.2, 0]], "a serve reset starts a new tail");
  assert.deepEqual(follow([], [0.5, 0.5], 3, 0.3), [[0.5, 0.5]]);
});
