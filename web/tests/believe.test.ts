import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { card_checkpoints, card_games, initSync, WebCards } from "../src/wasm/feint_web.js";
import {
  callIndex,
  cardRecord,
  cardsWord,
  eventText,
  moveFor,
  parseEvents,
  pick,
  ending,
  verdict,
  type CardEvent,
} from "../src/games/believe.ts";
import { cards, leaveCards, first, start } from "../src/screens.ts";

initSync({ module: readFileSync(new URL("../src/wasm/feint_web_bg.wasm", import.meta.url)) });

test("russian card counts take the right form", () => {
  const cases: [number, string][] = [
    [1, "карту"],
    [2, "карты"],
    [3, "карты"],
    [4, "карты"],
    [5, "карт"],
    [11, "карт"],
    [12, "карт"],
    [14, "карт"],
    [21, "карту"],
    [22, "карты"],
    [0, "карт"],
  ];
  for (const [n, word] of cases) {
    assert.equal(cardsWord(n), word, `${n}`);
  }
});

test("flat events parse by stride and read in russian", () => {
  const flat = [0, 1, 3, 2, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 3, 0];
  const events = parseEvents(flat, 9, 5);
  assert.deepEqual(events, [
    { kind: "lay", seat: 1, claim: 3, count: 2 },
    { kind: "call", seat: 0, honest: false, taker: 1, revealed: [0, 1, 0, 1, 0] },
    { kind: "call", seat: 1, honest: true, taker: 1, revealed: [0, 0, 0, 3, 0] },
  ]);
  assert.equal(eventText(events[0]), "Бот положил 2 карты — «дамы»");
  assert.equal(eventText({ kind: "lay", seat: 0, claim: 0, count: 1 }), "Вы положили 1 карту — «девятки»");
  assert.equal(
    eventText(events[1]),
    "Вы: «Не верю!» Открыто: 10, Д. Это был блеф — стопку забирает бот.",
  );
  assert.equal(
    eventText(events[2]),
    "Бот: «Не верю!» Открыто: Д ×3. Было честно — стопку забирает бот.",
  );
  const ending = parseEvents([1, 0, 1, -1, 0, 0, 3, 0, 0, 2, 1, 0, 0, 0, 0, 0, 0, 0], 9, 5);
  assert.deepEqual(ending, [
    { kind: "call", seat: 0, honest: true, taker: null, revealed: [0, 0, 3, 0, 0] },
    { kind: "out", seat: 1 },
  ]);
  assert.equal(eventText(ending[0]), "Вы: «Не верю!» Открыто: В ×3. Было честно — стопку никто не забирает.");
  assert.equal(eventText(ending[1]), "У бота не осталось карт");
  assert.equal(eventText({ kind: "out", seat: 0 }), "У вас не осталось карт");
  assert.deepEqual(parseEvents([], 9, 5), []);
  assert.deepEqual(parseEvents([0, 1, 3], 9, 5), [], "a partial stride is dropped");
});

test("the verdict names a draw as a draw", () => {
  assert.equal(verdict(1), "Вы выиграли");
  assert.equal(verdict(0), "Ничья");
  assert.equal(verdict(-1), "Бот выиграл");
  assert.equal(verdict(undefined), null);
});

test("a match cut by the tick cap is not a result and is not uploaded", () => {
  assert.deepEqual(ending(undefined), { title: "Время вышло — партия не закончена", upload: false });
  assert.deepEqual(ending(0), { title: "Ничья", upload: true });
  assert.deepEqual(ending(1), { title: "Вы выиграли", upload: true });
  assert.deepEqual(ending(-1), { title: "Бот выиграл", upload: true });
});

test("picking adds one card of a rank within the hand and the batch", () => {
  const hand = [1, 2, 0, 3, 2];
  let picked = [0, 0, 0, 0, 0];
  picked = pick(picked, hand, 1);
  picked = pick(picked, hand, 1);
  assert.deepEqual(picked, [0, 2, 0, 0, 0]);
  assert.deepEqual(pick(picked, hand, 1), picked, "no third ten in the hand");
  assert.deepEqual(pick(picked, hand, 2), picked, "no jack in the hand");
  picked = pick(picked, hand, 4);
  assert.deepEqual(picked, [0, 2, 0, 0, 1]);
  assert.deepEqual(pick(picked, hand, 3), picked, "a batch holds three cards at most");
  assert.deepEqual(pick(picked, hand, -1), picked);
  assert.deepEqual(pick(picked, hand, 5), picked);
  assert.deepEqual(pick([0, 0, 0, 0, 0], hand, 0), [1, 0, 0, 0, 0], "the last card of a rank");
});

test("a move is found only among the legal ones", () => {
  const table: Record<number, number[]> = { 0: [-1], 4: [2, 0, 1, 0, 0, 0], 9: [2, 0, 2, 0, 0, 0], 12: [3, 0, 1, 0, 0, 0] };
  const describe = (i: number) => table[i] ?? [];
  assert.equal(moveFor([0, 4, 9], describe, 2, [0, 1, 0, 0, 0]), 4);
  assert.equal(moveFor([0, 4, 9], describe, 2, [0, 2, 0, 0, 0]), 9);
  assert.equal(moveFor([0, 4, 9], describe, 3, [0, 1, 0, 0, 0]), null, "12 is not legal now");
  assert.equal(moveFor([0, 4, 9], describe, 2, [0, 0, 0, 0, 0]), null, "an empty batch");
  assert.equal(moveFor([0, 4, 9], describe, null, [0, 1, 0, 0, 0]), null, "no claim chosen");
  assert.equal(callIndex([0, 4, 9], describe), 0);
  assert.equal(callIndex([4, 9], describe), null);
});

test("the card screen opens from the hub and returns to it", () => {
  const hub = first("ann");
  assert.deepEqual(cards(hub, "  bob "), { name: "cards", nick: "bob" });
  const game = start(hub, "airhockey", 3, "ann");
  assert.equal(cards(game, "bob"), game, "not from a running match");
  assert.deepEqual(leaveCards({ name: "cards", nick: "bob" }), { name: "level", nick: "bob" });
  assert.equal(leaveCards(hub), hub);
});

test("a whole card match plays through the screen's helpers in wasm", () => {
  assert.deepEqual(Array.from(card_games()), ["believe"]);
  const checkpoint = card_checkpoints()[0];
  assert.match(checkpoint, /^[0-9a-f]{16}$/);
  assert.throws(() => new WebCards(1, "0000000000000001"));
  const m = new WebCards(0xbe1e, checkpoint);
  const describe = (i: number) => Array.from(m.describe(i));
  let ticks = 0;
  let mine = 0;
  while (!m.finished() && ticks < 36000) {
    if (m.my_turn()) {
      const hand = Array.from(m.hand());
      const claim = m.claim() ?? hand.findIndex((c) => c > 0);
      let picked = new Array(m.ranks()).fill(0);
      picked = pick(picked, hand, hand.findIndex((c) => c > 0));
      const lay = moveFor(Array.from(m.legal()), describe, claim, picked);
      const call = callIndex(Array.from(m.legal()), describe);
      const index = mine % 3 === 2 && call !== null ? call : lay ?? call;
      assert.notEqual(index, null, `no move at tick ${ticks}`);
      assert.ok(m.play(index as number));
      mine += 1;
    }
    m.advance(1);
    ticks += 1;
  }
  assert.ok(m.finished());
  assert.notEqual(verdict(m.outcome()), null);
  const events: CardEvent[] = parseEvents(Array.from(m.events()), m.event_stride(), m.ranks());
  assert.ok(events.some((e) => e.kind === "call"));
  events.forEach((e, i) => {
    if (e.kind === "call") {
      assert.equal(events[i - 1]?.kind, "lay", "a call answers a lay");
      assert.ok(eventText(e).includes("«Не верю!»"));
    }
  });
  assert.ok(m.record().length > 0);
  m.free();
});

test("the card record sums believe rows over levels and counts draws", () => {
  const row = (game: string, level: number, matches: number, won: number, lost: number) => ({
    game,
    level,
    matches,
    won,
    lost,
    mine: 0,
    theirs: 0,
  });
  assert.equal(
    cardRecord([row("believe", 5, 4, 1, 2), row("airhockey", 5, 9, 9, 0), row("believe", 3, 3, 2, 0)]),
    "Сыграно партий: 7 — побед 3, поражений 2, ничьих 2",
  );
  assert.equal(cardRecord([row("believe", 5, 1, 0, 1)]), "Сыграно партий: 1 — побед 0, поражений 1, ничьих 0");
});

test("no believe rows read as not played yet", () => {
  assert.equal(cardRecord([]), "Вы ещё не сыграли ни одной партии");
  assert.equal(
    cardRecord([{ game: "penalty", level: 5, matches: 3, won: 1, lost: 1, mine: 4, theirs: 4 }]),
    "Вы ещё не сыграли ни одной партии",
  );
});
