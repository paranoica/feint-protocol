import { test } from "node:test";
import assert from "node:assert/strict";
import { idle, press, release, steers } from "../src/drag.ts";

test("only a held primary pointer steers the mallet", () => {
  assert.equal(steers(idle, 1), false, "a hovering mouse steers nothing");
  const held = press(idle, 1, true);
  assert.equal(steers(held, 1), true);
  assert.equal(steers(held, 2), false, "a second finger does not steer");
  assert.equal(press(held, 2, true), held, "a second press does not take over");
  assert.equal(steers(release(held, 2), 1), true, "another pointer's release changes nothing");
  assert.equal(steers(release(held, 1), 1), false);
});

test("a secondary button does not start a drag", () => {
  assert.equal(press(idle, 1, false), idle);
});
