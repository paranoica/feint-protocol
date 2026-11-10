import { test } from "node:test";
import assert from "node:assert/strict";
import { parseComparison, parseResults, verdict, yours } from "../src/compare.ts";

const level = (n: number, share: number) => ({ level: n, matches: 40, won: 20, mine: 100, theirs: 50, share, fakes: 1.5 });
const good = {
  matches: 40,
  games: [{ id: "duel", levels: [1, 2, 3, 4, 5].map((n) => level(n, n / 10)), honest: level(5, 0.4), payoff: 0.1 }],
};

test("a measured comparison is read whole and anything malformed is refused", () => {
  const c = parseComparison(good);
  assert.equal(c?.games.length, 1);
  assert.equal(c?.games[0].levels[4].share, 0.5);
  assert.equal(c?.games[0].honest.share, 0.4);
  for (const broken of [
    null,
    [],
    { ...good, matches: -1 },
    { ...good, games: [{ ...good.games[0], levels: good.games[0].levels.slice(0, 4) }] },
    { ...good, games: [{ ...good.games[0], levels: [...good.games[0].levels].reverse() }] },
    { ...good, games: [{ ...good.games[0], honest: { ...level(5, 0.4), share: 1.5 } }] },
    { ...good, games: [{ ...good.games[0], payoff: "big" }] },
  ]) {
    assert.equal(parseComparison(broken), null, JSON.stringify(broken).slice(0, 80));
  }
});

test("results keep only well-formed rows and say what you played", () => {
  const rows = parseResults([
    { game: "duel", level: 5, matches: 8, won: 1, lost: 7, mine: 9, theirs: 38 },
    { game: "duel", level: 1, matches: 2, won: 1, lost: 1, mine: -1, theirs: 5 },
    "junk",
  ]);
  assert.equal(rows.length, 1);
  assert.equal(yours(rows, "duel", 5), "you 1–7 · 9:38");
  assert.equal(yours(rows, "duel", 1), "not played");
  assert.equal(yours(rows, "penalty", 5), "not played");
  assert.deepEqual(parseResults({ game: "duel" }), []);
});

test("the verdict reads the deception payoff with its sign and a dead zone", () => {
  assert.match(verdict(0.12), /^deception decides: level 5 gains 0\.12/);
  assert.match(verdict(0.05), /^deception decides/);
  assert.match(verdict(0.049), /^deception barely matters here \(\+0\.05\)/);
  assert.match(verdict(-0.03), /^deception barely matters here \(−0\.03\)/);
  assert.match(verdict(-0.08), /^deception costs: level 5 loses 0\.08/);
});
