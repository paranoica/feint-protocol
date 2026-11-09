import { test } from "node:test";
import assert from "node:assert/strict";
import { due, makeClock } from "../src/clock.ts";

test("whole ticks run, fractions carry, the cap drops the rest", () => {
  const c = makeClock();
  assert.equal(due(c, 1000, false, 240, 12), 0, "the first frame runs nothing");
  assert.equal(due(c, 1025, false, 240, 12), 6);
  assert.equal(due(c, 1027, false, 240, 12), 0);
  assert.equal(due(c, 1030, false, 240, 12), 1, "0.48 + 0.72 carries over one tick");
  assert.equal(due(c, 2030, false, 240, 12), 12, "a long stall runs only the cap");
  assert.equal(due(c, 2032, false, 240, 12), 0, "and drops the time beyond it");
});

test("a hidden tab runs nothing and restarts on return", () => {
  const c = makeClock();
  due(c, 0, false, 240, 12);
  assert.equal(due(c, 5000, true, 240, 12), 0);
  assert.equal(c.last, null);
  assert.equal(due(c, 9000, false, 240, 12), 0, "the first visible frame runs nothing");
  assert.equal(due(c, 9025, false, 240, 12), 6);
});

test("speed scales time and the tick rate comes from the caller", () => {
  const c = makeClock();
  due(c, 0, false, 240, 96, 8);
  assert.equal(due(c, 25, false, 240, 96, 8), 48);
  const slow = makeClock();
  due(slow, 0, false, 60, 12);
  assert.equal(due(slow, 100, false, 60, 12), 6);
});
