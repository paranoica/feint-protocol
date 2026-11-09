import { test } from "node:test";
import assert from "node:assert/strict";
import { fit, pixels, toCanvas, toTable } from "../src/geometry.ts";

const near = (a: number[], b: number[]) => a.every((v, i) => Math.abs(v - b[i]) < 1e-9);

test("the human's goal is at the bottom centre and the corners are fixed", () => {
  const f = fit({ width: 400, height: 800, dpr: 1 });
  assert.ok(near(toCanvas(f, -1, 0), [200, 800]), "own goal at the bottom centre");
  assert.ok(near(toCanvas(f, 1, 0), [200, 0]), "the bot's goal at the top centre");
  assert.ok(near(toCanvas(f, 1, 0.5), [0, 0]), "far left corner");
  assert.ok(near(toCanvas(f, 1, -0.5), [400, 0]), "far right corner");
  assert.ok(near(toCanvas(f, -1, -0.5), [400, 800]), "near right corner");
  assert.ok(near(toTable(f, 200, 400), [0, 0]), "centre spot");
});

test("a wide canvas letterboxes the table and keeps the mapping", () => {
  const f = fit({ width: 1000, height: 800, dpr: 1 });
  assert.deepEqual([f.scale, f.left, f.top], [400, 300, 0]);
  assert.ok(near(toCanvas(f, -1, 0), [500, 800]));
  assert.ok(near(toTable(f, 300, 0), [1, 0.5]));
});

test("device pixel ratio scales the buffer, not the mapping", () => {
  const one = fit({ width: 400, height: 800, dpr: 1 });
  const two = fit({ width: 400, height: 800, dpr: 2 });
  assert.deepEqual(one, two);
  assert.deepEqual(pixels({ width: 400, height: 800, dpr: 2 }), { width: 800, height: 1600 });
});

test("a pointer off the table clamps to its edge", () => {
  const f = fit({ width: 400, height: 800, dpr: 1 });
  assert.ok(near(toTable(f, -50, 2000), [-1, 0.5]));
  assert.ok(near(toTable(f, 900, -300), [1, -0.5]));
});
