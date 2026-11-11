import { test } from "node:test";
import assert from "node:assert/strict";
import { again, cleanNick, finish, first, start, watch } from "../src/screens.ts";

test("level, game with score, result, again", () => {
  const level = first("ann");
  const game = start(level, "airhockey", 3, "ann");
  assert.deepEqual(game, { name: "game", game: "airhockey", nick: "ann", level: 3 });
  const result = finish(game, [7, 4]);
  assert.deepEqual(result, { name: "result", game: "airhockey", nick: "ann", level: 3, score: [7, 4] });
  assert.deepEqual(again(result), { name: "level", nick: "ann" });
});

test("out-of-order moves and bad levels change nothing", () => {
  const level = first("ann");
  assert.equal(finish(level, [1, 2]), level);
  assert.equal(again(level), level);
  assert.equal(start(level, "airhockey", 0, "ann"), level);
  assert.equal(start(level, "airhockey", 6, "ann"), level);
  assert.equal(start(level, "airhockey", 2.5, "ann"), level);
  assert.equal(start(level, "", 2, "ann"), level, "a game is named");
  const game = start(level, "airhockey", 1, "ann");
  assert.equal(start(game, "airhockey", 2, "ann"), game);
});

test("nicks are trimmed, capped at 24 characters and never empty", () => {
  assert.equal(cleanNick("  ann  "), "ann");
  assert.equal(cleanNick(""), "player");
  assert.equal(cleanNick("    "), "player");
  assert.equal(cleanNick("x".repeat(30)), "x".repeat(24));
  assert.equal(cleanNick("ё".repeat(30)), "ё".repeat(24));
  assert.equal(cleanNick("🙂".repeat(25)).length, 48, "24 emoji, counted by code point");
});

test("control and direction characters are dropped from nicks", () => {
  assert.equal(cleanNick("a\u0007b"), "ab");
  assert.equal(cleanNick("a\u202eb\u2067c"), "abc");
  assert.equal(cleanNick("\u200f"), "player");
});

test("watching two bots goes from the menu to a result naming the winner", () => {
  const level = first("ann");
  const game = watch(level, "airhockey", 1, 5, "ann");
  assert.deepEqual(game, { name: "game", game: "airhockey", nick: "ann", level: 5, watch: [1, 5] });
  const result = finish(game, [2, 7]);
  assert.deepEqual(result, {
    name: "result",
    game: "airhockey",
    nick: "ann",
    level: 5,
    score: [2, 7],
    watch: [1, 5],
  });
  assert.deepEqual(again(result), { name: "level", nick: "ann" });
  assert.equal(watch(level, "airhockey", 0, 5, "ann"), level);
  assert.equal(watch(level, "airhockey", 3, 6, "ann"), level);
  assert.equal(watch(level, "", 1, 2, "ann"), level);
  assert.equal(watch(game, "airhockey", 1, 2, "ann"), game, "only from the menu");
  const human = start(level, "airhockey", 2, "ann");
  assert.equal(human.name === "game" ? human.watch : null, undefined, "a human game is not a watch");
});

test("a trained start keeps the bot kind through finish and again", () => {
  const game = start(first("ann"), "airhockey", 4, "ann", "trained");
  assert.deepEqual(game, { name: "game", game: "airhockey", nick: "ann", level: 4, bot: "trained" });
  const result = finish(game, [2, 7]);
  assert.deepEqual(result, {
    name: "result",
    game: "airhockey",
    nick: "ann",
    level: 4,
    score: [2, 7],
    bot: "trained",
  });
  assert.deepEqual(again(result), { name: "level", nick: "ann" });
  assert.deepEqual(start(first("ann"), "airhockey", 4, "ann", "hand-made"), start(first("ann"), "airhockey", 4, "ann"));
});

test("start refuses an unknown bot kind", () => {
  const level = first("ann");
  assert.equal(start(level, "airhockey", 3, "ann", "neural" as never), level);
  assert.equal(start(level, "airhockey", 3, "ann", "" as never), level);
});
