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

test("a wide screen lays the table across with the human on the left", () => {
  const f = fit({ width: 1000, height: 800, dpr: 1 });
  assert.equal(f.horizontal, true);
  assert.deepEqual([f.scale, f.left, f.top], [500, 0, 150]);
  assert.ok(near(toCanvas(f, -1, 0), [0, 400]), "own goal at the left centre");
  assert.ok(near(toCanvas(f, 1, 0), [1000, 400]), "the bot's goal at the right centre");
  assert.ok(near(toCanvas(f, -1, 0.5), [0, 150]), "the human's left is up");
  assert.ok(near(toTable(f, 1000, 650), [1, -0.5]));
  assert.ok(near(toTable(f, 500, 400), [0, 0]), "centre spot");
});

test("a tall screen keeps the table upright", () => {
  const f = fit({ width: 400, height: 800, dpr: 1 });
  assert.equal(f.horizontal, false);
  const square = fit({ width: 600, height: 600, dpr: 1 });
  assert.equal(square.horizontal, false, "a square screen stays upright");
});

test("padding keeps room around the table and the mapping still inverts", () => {
  const f = fit({ width: 1000, height: 600, dpr: 1 }, undefined, 50);
  assert.deepEqual([f.scale, f.left, f.top], [450, 50, 75]);
  for (const [x, y] of [[-1, 0.5], [0.3, -0.2], [1, -0.5]]) {
    const [cx, cy] = toCanvas(f, x, y);
    assert.ok(near(toTable(f, cx, cy), [x, y]), `${x}, ${y}`);
  }
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
  const wide = fit({ width: 1000, height: 800, dpr: 1 });
  assert.ok(near(toTable(wide, -50, -300), [-1, 0.5]));
  assert.ok(near(toTable(wide, 2000, 2000), [1, -0.5]));
});
