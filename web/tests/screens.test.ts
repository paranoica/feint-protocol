import { test } from "node:test";
import assert from "node:assert/strict";
import { again, cleanNick, finish, first, start } from "../src/screens.ts";

test("level, game with score, result, again", () => {
  const level = first("ann");
  const game = start(level, 3, "ann");
  assert.deepEqual(game, { name: "game", nick: "ann", level: 3 });
  const result = finish(game, [7, 4]);
  assert.deepEqual(result, { name: "result", nick: "ann", level: 3, score: [7, 4] });
  assert.deepEqual(again(result), { name: "level", nick: "ann" });
});

test("out-of-order moves and bad levels change nothing", () => {
  const level = first("ann");
  assert.equal(finish(level, [1, 2]), level);
  assert.equal(again(level), level);
  assert.equal(start(level, 0, "ann"), level);
  assert.equal(start(level, 6, "ann"), level);
  assert.equal(start(level, 2.5, "ann"), level);
  const game = start(level, 1, "ann");
  assert.equal(start(game, 2, "ann"), game);
});

test("nicks are trimmed, capped at 24 characters and never empty", () => {
  assert.equal(cleanNick("  ann  "), "ann");
  assert.equal(cleanNick(""), "player");
  assert.equal(cleanNick("    "), "player");
  assert.equal(cleanNick("x".repeat(30)), "x".repeat(24));
  assert.equal(cleanNick("ё".repeat(30)), "ё".repeat(24));
  assert.equal(cleanNick("🙂".repeat(25)).length, 48, "24 emoji, counted by code point");
});
