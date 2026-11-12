import { test } from "node:test";
import assert from "node:assert/strict";
import { fakeLinks, fakesLine, parseComparison, parseFakes, parseResults, verdict, yours } from "../src/compare.ts";

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
  assert.match(verdict(0.049), /^deception decides: faked attacks score 5 points more/, "the verdict follows the rounded points it prints");
  assert.match(verdict(0.044), /^deception barely matters here: faked and honest attacks score within 4 points/);
  assert.match(verdict(-0.03), /^deception barely matters here: .* within 3 points/);
  assert.match(verdict(-0.08), /^deception costs: faked attacks score 8 points less/);
  assert.match(verdict(null), /^deception untested here: fewer than 20/);
});

test("the gain is the faked scoring rate minus the honest one, and needs both", async () => {
  const { attackGain, attackLine } = await import("../src/compare.ts");
  const c = parseComparison(good)!;
  assert.equal(attackGain(c.games[0]), null, "4 faked attacks are too few to judge");
  const many = { ...c.games[0], attacks: { honest: { count: 40, scored: 12 }, fake: { count: 20, scored: 10, answered: 15, bitten: 5 } } };
  assert.ok(Math.abs(attackGain(many)! - (10 / 20 - 12 / 40)) < 1e-12);
  const few = { ...many, attacks: { ...many.attacks, honest: { count: 19, scored: 1 } } };
  assert.equal(attackGain(few), null);
  const unanswered = { ...many, attacks: { ...many.attacks, fake: { count: 20, scored: 10, answered: 0, bitten: 0 } } };
  assert.match(attackLine(unanswered), /; no fake was answered$/);
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
    "level 5 attacks score 84% honest (32) and 96% with a fake (127); the human model bit on 18% of the 127 fakes it answered",
  );
  assert.equal(parseComparison({ ...withAttacks, games: [{ ...withAttacks.games[0], attacks: { honest: { count: 1, scored: 2 }, fake: withAttacks.games[0].attacks.fake } }] }), null, "more goals than attacks");
  const none = parseComparison({ ...good, games: [{ ...good.games[0], attacks: { honest: { count: 0, scored: 0 }, fake: { count: 0, scored: 0, answered: 0, bitten: 0 } } }] });
  assert.equal(attackLine(none!.games[0]), "level 5 made no attacks");
});

const fakes = {
  level: 5,
  checkpoint: "15171d6c57a0caa7",
  matches: 200,
  noise: 0.49,
  shots: 16134,
  fake: { count: 7, scored: 0, low: 0.0, high: 0.3543 },
  honest: { count: 16127, scored: 1242, low: 0.073, high: 0.0812 },
  links: [
    { seed: 30030, seat: 0, from: 5748, scored: false },
    { seed: 30081, seat: 1, from: 6953, scored: false },
  ],
};

test("the trained bot's fakes are read whole and anything malformed is refused", () => {
  const f = parseFakes(fakes);
  assert.equal(f?.fake.count, 7);
  assert.equal(f?.links.length, 2);
  assert.deepEqual(f?.links[1], { seed: 30081, seat: 1, from: 6953, scored: false });
  const none = parseFakes({ ...fakes, shots: 16127, fake: { count: 0, scored: 0, low: null, high: null }, links: [] });
  assert.equal(none?.fake.low, null);
  const link = fakes.links[0];
  for (const broken of [
    null,
    [],
    { ...fakes, level: 6 },
    { ...fakes, checkpoint: "xyz" },
    { ...fakes, matches: -1 },
    { ...fakes, fake: { ...fakes.fake, scored: 8 } },
    { ...fakes, fake: { ...fakes.fake, high: 1.5 } },
    { ...fakes, fake: { ...fakes.fake, low: "0" } },
    { ...fakes, honest: undefined },
    { ...fakes, shots: 10 },
    { ...fakes, links: [link, link, link, link] },
    { ...fakes, links: [{ ...link, seat: 2 }] },
    { ...fakes, links: [{ ...link, seed: 4294967296 }] },
    { ...fakes, links: [{ ...link, from: -1 }] },
    { ...fakes, links: [{ ...link, from: 144001 }] },
    { ...fakes, noise: -0.1 },
    { ...fakes, noise: "0.49" },
    { ...fakes, noise: Infinity },
    { ...fakes, links: [{ ...link, scored: "no" }] },
    { ...fakes, links: "none" },
  ]) {
    assert.equal(parseFakes(broken), null, JSON.stringify(broken));
  }
});

test("the fake line says how often faked and honest attacks score, or that none were found", () => {
  const f = parseFakes(fakes);
  assert.ok(f);
  assert.equal(
    fakesLine(f),
    "trained level 5 against the human model, 200 matches: 7 of 16134 shots look like fakes, 0.04 a match, no more than the 0.49 a match the detector finds in a bot that never fakes: no sign of fakes",
  );
  const many = parseFakes({ ...fakes, fake: { ...fakes.fake, count: 200, scored: 30 }, shots: 16327 });
  assert.ok(many);
  assert.equal(
    fakesLine(many),
    "trained level 5 against the human model, 200 matches: 200 faked attacks of 16327 shots, 1.00 a match against 0.49 of false alarms, scored 15% (95% up to 35%), honest ones 8%",
  );
  const none = parseFakes({ ...fakes, shots: 16127, fake: { count: 0, scored: 0, low: null, high: null }, links: [] });
  assert.ok(none);
  assert.equal(
    fakesLine(none),
    "trained level 5 against the human model, 200 matches: no faked attack among 16127 shots",
  );
});

test("replay links are offered only for the checkpoint this build ships", () => {
  const f = parseFakes(fakes);
  assert.ok(f);
  const shipped = ["11e0e1c1a09628b6", "3ac002e12274846f", "3ac002e12274846f", "3ac002e12274846f", "15171d6c57a0caa7"];
  assert.equal(fakeLinks(f, shipped).length, 2);
  assert.deepEqual(fakeLinks(f, shipped.slice(0, 4).concat("0000000000000001")), []);
  assert.deepEqual(fakeLinks(f, []), []);
});
