export const RANK_LABELS = ["9", "10", "В", "Д", "К"];
export const RANK_PLURAL = ["девятки", "десятки", "валеты", "дамы", "короли"];
export const MAX_BATCH = 3;
export const CARD_TICK_MS = 100;

export type CardEvent =
  | { kind: "lay"; seat: 0 | 1; claim: number; count: number }
  | { kind: "call"; seat: 0 | 1; honest: boolean; taker: 0 | 1 | null; revealed: number[] }
  | { kind: "out"; seat: 0 | 1 };

const seatOf = (v: number): 0 | 1 => (v === 0 ? 0 : 1);
const label = (rank: number) => RANK_LABELS[rank] ?? String(rank);
const plural = (rank: number) => RANK_PLURAL[rank] ?? String(rank);

export function parseEvents(flat: ArrayLike<number>, stride: number, ranks: number): CardEvent[] {
  const out: CardEvent[] = [];
  for (let at = 0; stride > 0 && at + stride <= flat.length; at += stride) {
    const [kind, seat, a, b] = [flat[at], flat[at + 1], flat[at + 2], flat[at + 3]];
    if (kind === 0) {
      out.push({ kind: "lay", seat: seatOf(seat), claim: a, count: b });
    } else if (kind === 2) {
      out.push({ kind: "out", seat: seatOf(seat) });
    } else {
      const revealed = Array.from({ length: ranks }, (_, r) => flat[at + 4 + r]);
      const taker = b < 0 ? null : seatOf(b);
      out.push({ kind: "call", seat: seatOf(seat), honest: a === 1, taker, revealed });
    }
  }
  return out;
}

export function cardsWord(n: number): string {
  const last = n % 10;
  const tens = n % 100;
  if (last === 1 && tens !== 11) {
    return "карту";
  }
  if (last >= 2 && last <= 4 && (tens < 12 || tens > 14)) {
    return "карты";
  }
  return "карт";
}

function revealedText(revealed: number[]): string {
  return revealed
    .map((count, rank) => (count === 0 ? "" : count === 1 ? label(rank) : `${label(rank)} ×${count}`))
    .filter((s) => s !== "")
    .join(", ");
}

export function eventText(e: CardEvent): string {
  if (e.kind === "lay") {
    const who = e.seat === 0 ? "Вы положили" : "Бот положил";
    return `${who} ${e.count} ${cardsWord(e.count)} — «${plural(e.claim)}»`;
  }
  if (e.kind === "out") {
    return e.seat === 0 ? "У вас не осталось карт" : "У бота не осталось карт";
  }
  const who = e.seat === 0 ? "Вы" : "Бот";
  const truth = e.honest ? "Было честно" : "Это был блеф";
  const taker = e.taker === null ? "никто не забирает" : e.taker === 0 ? "забираете вы" : "забирает бот";
  return `${who}: «Не верю!» Открыто: ${revealedText(e.revealed)}. ${truth} — стопку ${taker}.`;
}

export function verdict(outcome: number | undefined): string | null {
  if (outcome === undefined) {
    return null;
  }
  return outcome > 0 ? "Вы выиграли" : outcome < 0 ? "Бот выиграл" : "Ничья";
}

export function pick(picked: number[], hand: number[], rank: number): number[] {
  const total = picked.reduce((a, b) => a + b, 0);
  if (rank >= hand.length || total >= MAX_BATCH || picked[rank] >= hand[rank]) {
    return picked;
  }
  return picked.map((c, r) => (r === rank ? c + 1 : c));
}

const same = (a: number[], b: number[]) => a.length === b.length && a.every((v, i) => v === b[i]);

export function moveFor(
  legal: number[],
  describe: (index: number) => number[],
  claim: number | null,
  picked: number[],
): number | null {
  if (claim === null || picked.every((c) => c === 0)) {
    return null;
  }
  const want = [claim, ...picked];
  return legal.find((i) => same(describe(i), want)) ?? null;
}

export function callIndex(legal: number[], describe: (index: number) => number[]): number | null {
  return legal.find((i) => same(describe(i), [-1])) ?? null;
}

export interface CardSource {
  ranks(): number;
  hand(): Uint8Array;
  claim(): number | undefined;
  pile(): number;
  last_count(): number;
  their_cards(): number;
  batches(): number;
  actions_left(): number;
  my_turn(): boolean;
  outcome(): number | undefined;
  event_stride(): number;
  events(): Int32Array;
  legal(): Int32Array;
  describe(index: number): Int32Array;
  play(index: number): boolean;
  finished(): boolean;
}

export const HOW_TO_PLAY = [
  "У вас и у бота по 8 карт пяти рангов: 9, 10, В, Д, К.",
  "Кладёте рубашкой вверх от 1 до 3 карт и говорите, что это за ранг. Можно соврать.",
  "Пока раунд идёт, ранг не меняется: каждый следующий кладёт карты «того же ранга» или говорит «Не верю!».",
  "«Не верю!» открывает последнюю партию. Соврали — стопку забирает тот, кто клал. Правда — забирает тот, кто не поверил.",
  "Выигрывает тот, кто первым избавится от карт и не попадётся. После 300 ходов побеждает тот, у кого карт меньше.",
];

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls: string, text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = cls;
  node.textContent = text;
  return node;
}

export class CardBoard {
  private picked: number[] = [];
  private chosen: number | null = null;
  private shown = "";
  private readonly root: HTMLElement;
  private readonly source: CardSource;

  constructor(root: HTMLElement, source: CardSource) {
    this.root = root;
    this.source = source;
    this.picked = new Array(source.ranks()).fill(0);
  }

  private describe = (i: number) => Array.from(this.source.describe(i));

  private reset() {
    this.picked = new Array(this.source.ranks()).fill(0);
    this.chosen = null;
  }

  private key(): string {
    const s = this.source;
    return [Array.from(s.hand()).join(), s.my_turn(), s.events().length, s.finished(), this.picked.join(), this.chosen].join("|");
  }

  render(force = false) {
    const key = this.key();
    if (!force && key === this.shown) {
      return;
    }
    this.shown = key;
    const s = this.source;
    const hand = Array.from(s.hand());
    const turn = s.my_turn();
    const standing = s.claim() ?? null;
    const claim = standing ?? this.chosen;
    const legal = turn ? Array.from(s.legal()) : [];
    const events = parseEvents(Array.from(s.events()), s.event_stride(), s.ranks());

    const status = el("div", "cards-status");
    status.append(
      el("span", "", `У бота: ${s.their_cards()} ${cardsWord(s.their_cards())}`),
      el("span", "", `В стопке: ${s.pile()}`),
      el("span", "", standing === null ? "Раунд не начат" : `Заявлено: «${plural(standing)}»`),
      el("span", "", `Ходов до конца: ${s.actions_left()}`),
    );

    const prompt = el(
      "p",
      "cards-turn",
      s.finished() ? "" : turn ? (standing === null ? "Ваш ход: выберите карты и ранг" : "Ваш ход: доложите или не поверьте") : "Ход бота…",
    );

    const handRow = el("div", "cards-hand");
    hand.forEach((count, rank) => {
      const b = el("button", "cards-rank");
      b.dataset.rank = String(rank);
      b.disabled = !turn || count === 0 || this.picked[rank] >= count;
      b.append(el("b", "", label(rank)), el("span", "", `×${count}`));
      if (this.picked[rank] > 0) {
        b.append(el("i", "", `+${this.picked[rank]}`));
      }
      b.addEventListener("click", () => {
        this.picked = pick(this.picked, hand, rank);
        if (this.chosen === null && standing === null) {
          this.chosen = rank;
        }
        this.render();
      });
      handRow.append(b);
    });

    const claimRow = el("div", "cards-claim");
    if (turn && standing === null) {
      claimRow.append(el("span", "", "Говорю, что это:"));
      RANK_PLURAL.slice(0, s.ranks()).forEach((name, rank) => {
        const b = el("button", "", name);
        b.setAttribute("aria-pressed", String(this.chosen === rank));
        b.addEventListener("click", () => {
          this.chosen = rank;
          this.render();
        });
        claimRow.append(b);
      });
    }

    const actions = el("div", "cards-actions");
    const lay = moveFor(legal, this.describe, claim, this.picked);
    const call = callIndex(legal, this.describe);
    const count = this.picked.reduce((a, b) => a + b, 0);
    const layButton = el(
      "button",
      "primary",
      count === 0 || claim === null ? "Положить" : `Положить ${count} ${cardsWord(count)} как «${plural(claim)}»`,
    );
    layButton.disabled = lay === null;
    layButton.addEventListener("click", () => {
      if (lay !== null && this.source.play(lay)) {
        this.reset();
        this.render(true);
      }
    });
    const callButton = el("button", "cards-call", "Не верю!");
    callButton.disabled = call === null;
    callButton.addEventListener("click", () => {
      if (call !== null && this.source.play(call)) {
        this.reset();
        this.render(true);
      }
    });
    const clear = el("button", "", "Сбросить выбор");
    clear.disabled = count === 0;
    clear.addEventListener("click", () => {
      this.reset();
      this.render();
    });
    if (turn) {
      actions.append(layButton, callButton, clear);
    }

    const log = el("ol", "cards-log");
    for (const e of events.slice(-8).reverse()) {
      const item = el("li", e.kind === "lay" ? "" : "call", eventText(e));
      item.classList.add(e.seat === 0 ? "you" : "bot");
      log.append(item);
    }

    this.root.replaceChildren(status, prompt, handRow, claimRow, actions, log);
  }
}
