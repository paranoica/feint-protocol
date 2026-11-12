import { test } from "node:test";
import assert from "node:assert/strict";
import { uploadPath } from "../src/api.ts";
import { profileWanted, trainedCheckpoint, trainedOffered } from "../src/bots.ts";

const FIVE = ["000000000000000a", "000000000000000b", "000000000000000c", "000000000000000d", "000000000000000e"];

test("each trained level plays its own checkpoint, listed in level order", () => {
  assert.equal(trainedCheckpoint("airhockey", FIVE, 1), "000000000000000a");
  assert.equal(trainedCheckpoint("airhockey", FIVE, 3), "000000000000000c");
  assert.equal(trainedCheckpoint("airhockey", FIVE, 5), "000000000000000e");
});

test("only air hockey offers the trained bot, and only with a checkpoint for the level", () => {
  assert.equal(trainedCheckpoint("airhockey", [], 1), null);
  assert.equal(trainedCheckpoint("airhockey", FIVE.slice(0, 4), 5), null);
  assert.equal(trainedCheckpoint("airhockey", FIVE, 0), null);
  assert.equal(trainedCheckpoint("airhockey", FIVE, 6), null);
  assert.equal(trainedCheckpoint("penalty", FIVE, 1), null);
  assert.equal(trainedCheckpoint("duel", FIVE, 5), null);
  assert.equal(trainedOffered("airhockey", FIVE), true);
  assert.equal(trainedOffered("airhockey", FIVE.slice(0, 4)), false);
  assert.equal(trainedOffered("penalty", FIVE), false);
});

test("a trained match never asks for a profile and uploads without one", () => {
  assert.equal(profileWanted("hand-made", 5, true), true);
  assert.equal(profileWanted("trained", 5, true), false);
  assert.equal(profileWanted("hand-made", 4, true), false);
  assert.equal(profileWanted("hand-made", 5, false), false);
  for (let level = 1; level <= 5; level++) {
    assert.equal(profileWanted("trained", level, true), false);
  }
  assert.ok(!uploadPath("ann", "t1", null).includes("profile="));
});
