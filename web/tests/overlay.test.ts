import { test } from "node:test";
import assert from "node:assert/strict";
import { describe, intentColor, intentName } from "../src/overlay.ts";

test("intents read as fake, strike, defend", () => {
  assert.equal(intentName(-1), "waiting");
  assert.equal(intentName(2), "strike");
  assert.equal(intentName(3), "fake");
  assert.equal(intentName(4), "defend");
});

test("the overlay says when there is no model and names predictions", () => {
  assert.deepEqual(describe({ intent: 3, memory: false, likelyShot: -1, likelyCover: [], shown: 0, real: 2 }), [
    "bot: fake",
    "showing left, playing right",
    "model: none at this level",
  ]);
  const lines = describe({ intent: 2, memory: true, likelyShot: 2, likelyCover: [0, -1, 2], shown: -1, real: -1 });
  assert.deepEqual(lines, [
    "bot: strike",
    "your next shot: left",
    "bot shows left → you cover left",
    "bot shows centre → you cover unsure",
    "bot shows right → you cover right",
  ]);
});

test("the bot's classes are named from the human's side of the table", () => {
  const lines = describe({ intent: 2, memory: false, likelyShot: -1, likelyCover: [], shown: 2, real: 2 });
  assert.deepEqual(lines, ["bot: strike", "showing right, playing right", "model: none at this level"]);
});

test("a strike that shows nothing says so", () => {
  const lines = describe({ intent: 2, memory: false, likelyShot: -1, likelyCover: [], shown: -1, real: 1 });
  assert.equal(lines[1], "showing nothing, playing centre");
});

test("the bot's colour in the debug view follows its intent", () => {
  assert.equal(intentColor(3), "#facc15", "a fake is yellow");
  assert.equal(intentColor(2), "#ef4444", "an honest strike is red");
  assert.equal(intentColor(4), "#a78bfa", "defending is violet");
  assert.equal(intentColor(0), "#94a3b8", "moving is grey");
  assert.equal(intentColor(-1), "#94a3b8", "waiting is grey");
  assert.equal(new Set([3, 2, 4, 0].map(intentColor)).size, 4, "four distinct colours");
});
