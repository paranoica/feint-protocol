import { test } from "node:test";
import assert from "node:assert/strict";
import { uploadPath } from "../src/api.ts";
import { profileWanted, trainedCheckpoint } from "../src/bots.ts";

test("only air hockey offers the trained bot, and only with a checkpoint", () => {
  assert.equal(trainedCheckpoint("airhockey", ["15171d6c57a0caa7"]), "15171d6c57a0caa7");
  assert.equal(trainedCheckpoint("airhockey", ["15171d6c57a0caa7", "0000000000000001"]), "15171d6c57a0caa7");
  assert.equal(trainedCheckpoint("airhockey", []), null);
  assert.equal(trainedCheckpoint("penalty", ["15171d6c57a0caa7"]), null);
  assert.equal(trainedCheckpoint("duel", ["15171d6c57a0caa7"]), null);
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
