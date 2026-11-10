import { test } from "node:test";
import assert from "node:assert/strict";
import { parseComparison, parseResults, verdict, yours } from "../src/compare.ts";

const level = (n: number, share: number) => ({ level: n, matches: 40, won: 20, mine: 100, theirs: 50, share, fakes: 1.5 });
const good = {
  matches: 40,
  games: [
    {
      id: "duel",
      levels: [1, 2, 3, 4, 5].map((n) => level(n, n / 10)),
      honest: level(5, 0.4),
      payoff: 0.1,
      attacks: { honest: { count: 10, scored: 3 }, fake: { count: 4, scored: 2, answered: 3, bitten: 1 } },
    },
  ],
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

test("the verdict compares faked and honest attacks with a dead zone", () => {
  assert.match(verdict(0.12), /^deception decides: faked attacks score 12 points more/);
  assert.match(verdict(0.05), /^deception decides/);
  assert.match(verdict(0.049), /^deception barely matters here: faked and honest attacks score within 5 points/);
  assert.match(verdict(-0.03), /^deception barely matters here: .* within 3 points/);
  assert.match(verdict(-0.08), /^deception costs: faked attacks score 8 points less/);
  assert.match(verdict(null), /^deception untested here/);
});

test("the gain is the faked scoring rate minus the honest one, and needs both", async () => {
  const { attackGain } = await import("../src/compare.ts");
  const c = parseComparison(good)!;
  assert.ok(Math.abs(attackGain(c.games[0])! - (2 / 4 - 3 / 10)) < 1e-12);
  const lone = { ...c.games[0], attacks: { ...c.games[0].attacks, fake: { count: 0, scored: 0, answered: 0, bitten: 0 } } };
  assert.equal(attackGain(lone), null);
});

test("attacks are read and told as scoring rates with and without a fake", async () => {
  const { attackLine } = await import("../src/compare.ts");
  const withAttacks = {
    ...good,
    games: [
      {
        ...good.games[0],
        attacks: { honest: { count: 32, scored: 27 }, fake: { count: 127, scored: 122, answered: 127, bitten: 23 } },
      },
    ],
  };
  const c = parseComparison(withAttacks);
  assert.ok(c);
  assert.equal(
    attackLine(c.games[0]),
    "level 5 attacks score 84% honest (32) and 96% with a fake (127); the human model bit on 18% of fakes",
  );
  assert.equal(parseComparison({ ...withAttacks, games: [{ ...withAttacks.games[0], attacks: { honest: { count: 1, scored: 2 }, fake: withAttacks.games[0].attacks.fake } }] }), null, "more goals than attacks");
  const none = parseComparison({ ...good, games: [{ ...good.games[0], attacks: { honest: { count: 0, scored: 0 }, fake: { count: 0, scored: 0, answered: 0, bitten: 0 } } }] });
  assert.equal(attackLine(none!.games[0]), "level 5 made no attacks");
});
