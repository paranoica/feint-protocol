import { test } from "node:test";
import assert from "node:assert/strict";

const host = globalThis as Record<string, unknown>;
host.document ??= { documentElement: {} };
host.getComputedStyle ??= () => ({ getPropertyValue: () => "" });
const { describeDuelist, readGunners, stepFromKey } = await import("../src/games/duel.ts");

test("Q and E or the arrows step aside, other keys leave the debug and speed keys alone", () => {
  assert.equal(stepFromKey("q"), -1);
  assert.equal(stepFromKey("E"), 1);
  assert.equal(stepFromKey("ArrowLeft"), -1);
  assert.equal(stepFromKey("ArrowRight"), 1);
  for (const key of ["d", "D", "f", "F", " ", "a"]) {
    assert.equal(stepFromKey(key), 0, key);
  }
});

test("the view is read seat by seat", () => {
  const v = [1, 0.5, -1, 0.8, 1, 1, 0, 2, 0.1, 0, 0, 0.4, 0, 0, 0, 1, 0.5, 0.3, -1, 3];
  const [me, them] = readGunners(v);
  assert.deepEqual(me, { x: -1, hand: 0.8, committed: true, loaded: true, recovery: 0 });
  assert.deepEqual(them, { x: 2, hand: 0.1, committed: false, loaded: false, recovery: 0.4 });
});

test("the overlay names fakes, draws and early or late steps", () => {
  const lines = describeDuelist("bot", {
    intent: 3,
    memory: true,
    likelyShot: 2,
    likelyCover: [0, 4, -1, -1, -1],
    shown: 0,
    real: -1,
  });
  assert.deepEqual(lines, [
    "bot: fake",
    "the other usually leads right",
    "on a fake the other steps left early",
    "on a draw the other steps right late",
  ]);
  assert.deepEqual(describeDuelist("bot", { intent: -1, memory: false, likelyShot: -1, likelyCover: [], shown: -1, real: -1 }), [
    "bot: waiting",
    "model: none at this level",
  ]);
});
