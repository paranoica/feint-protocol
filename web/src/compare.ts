export interface LevelRow {
  level: number;
  matches: number;
  won: number;
  mine: number;
  theirs: number;
  share: number;
  fakes: number;
}

export interface Attacks {
  honest: { count: number; scored: number };
  fake: { count: number; scored: number; answered: number; bitten: number };
}

export interface GameRow {
  id: string;
  levels: LevelRow[];
  honest: LevelRow;
  payoff: number;
  attacks: Attacks;
}

export interface Comparison {
  matches: number;
  games: GameRow[];
}

export interface FakeSide {
  count: number;
  scored: number;
  low: number | null;
  high: number | null;
}

export interface FakeLink {
  seed: number;
  seat: 0 | 1;
  from: number;
  scored: boolean;
}

export interface Fakes {
  level: number;
  checkpoint: string;
  matches: number;
  shots: number;
  fake: FakeSide;
  honest: FakeSide;
  links: FakeLink[];
}

export interface Result {
  game: string;
  level: number;
  matches: number;
  won: number;
  lost: number;
  mine: number;
  theirs: number;
}

const isCount = (v: unknown): v is number => typeof v === "number" && Number.isInteger(v) && v >= 0;
const isShare = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v) && v >= 0 && v <= 1;
const record = (v: unknown): Record<string, unknown> | null =>
  typeof v === "object" && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : null;

function levelRow(v: unknown): LevelRow | null {
  const r = record(v);
  if (!r) {
    return null;
  }
  const { level, matches, won, mine, theirs, share, fakes } = r;
  const fine =
    isCount(level) &&
    level >= 1 &&
    level <= 5 &&
    isCount(matches) &&
    isCount(won) &&
    isCount(mine) &&
    isCount(theirs) &&
    isShare(share) &&
    typeof fakes === "number" &&
    Number.isFinite(fakes) &&
    fakes >= 0;
  return fine ? { level, matches, won, mine, theirs, share, fakes } : null;
}

function attacksOf(v: unknown): Attacks | null {
  const r = record(v);
  const honest = record(r?.honest);
  const fake = record(r?.fake);
  if (!honest || !fake) {
    return null;
  }
  const counts = [honest.count, honest.scored, fake.count, fake.scored, fake.answered, fake.bitten];
  if (!counts.every(isCount)) {
    return null;
  }
  const [hc, hs, fc, fs, fa, fb] = counts as number[];
  if (hs > hc || fs > fc || fa > fc || fb > fa) {
    return null;
  }
  return { honest: { count: hc, scored: hs }, fake: { count: fc, scored: fs, answered: fa, bitten: fb } };
}

export function parseComparison(json: unknown): Comparison | null {
  const r = record(json);
  if (!r || !isCount(r.matches) || !Array.isArray(r.games)) {
    return null;
  }
  const games: GameRow[] = [];
  for (const g of r.games) {
    const row = record(g);
    if (!row || typeof row.id !== "string" || !Array.isArray(row.levels) || typeof row.payoff !== "number") {
      return null;
    }
    const levels = row.levels.map(levelRow);
    const honest = levelRow(row.honest);
    const attacks = attacksOf(row.attacks);
    if (levels.length !== 5 || levels.some((l, i) => l === null || l.level !== i + 1) || !honest || !attacks) {
      return null;
    }
    games.push({ id: row.id, levels: levels as LevelRow[], honest, payoff: row.payoff, attacks });
  }
  return { matches: r.matches, games };
}

const MAX_LINKS = 3;
const MAX_SEED = 0xffffffff;

function sideOf(v: unknown): FakeSide | null {
  const r = record(v);
  if (!r || !isCount(r.count) || !isCount(r.scored) || r.scored > r.count) {
    return null;
  }
  const bound = (b: unknown) => b === null || isShare(b);
  if (!bound(r.low) || !bound(r.high)) {
    return null;
  }
  return { count: r.count, scored: r.scored, low: r.low as number | null, high: r.high as number | null };
}

function linkOf(v: unknown): FakeLink | null {
  const r = record(v);
  if (!r || !isCount(r.seed) || r.seed > MAX_SEED || (r.seat !== 0 && r.seat !== 1) || !isCount(r.from)) {
    return null;
  }
  return typeof r.scored === "boolean" ? { seed: r.seed, seat: r.seat, from: r.from, scored: r.scored } : null;
}

export function parseFakes(json: unknown): Fakes | null {
  const r = record(json);
  if (!r || !isCount(r.level) || r.level < 1 || r.level > 5 || !isCount(r.matches) || !isCount(r.shots)) {
    return null;
  }
  if (typeof r.checkpoint !== "string" || !/^[0-9a-f]{16}$/.test(r.checkpoint) || !Array.isArray(r.links)) {
    return null;
  }
  const fake = sideOf(r.fake);
  const honest = sideOf(r.honest);
  const links = r.links.map(linkOf);
  if (!fake || !honest || fake.count + honest.count !== r.shots || links.length > MAX_LINKS) {
    return null;
  }
  if (links.some((l) => l === null)) {
    return null;
  }
  return {
    level: r.level,
    checkpoint: r.checkpoint,
    matches: r.matches,
    shots: r.shots,
    fake,
    honest,
    links: links as FakeLink[],
  };
}

export function fakesLine(f: Fakes): string {
  const head = `trained level ${f.level} against the human model, ${f.matches} matches`;
  if (f.fake.count === 0) {
    return `${head}: no faked attack among ${f.shots} shots`;
  }
  const pct = (a: number, b: number) => (b === 0 ? "—" : `${Math.round((100 * a) / b)}%`);
  const up = f.fake.high === null ? "" : ` (95% up to ${Math.round(100 * f.fake.high)}%)`;
  return `${head}: ${f.fake.count} faked attacks of ${f.shots} shots scored ${pct(f.fake.scored, f.fake.count)}${up}, honest ones ${pct(f.honest.scored, f.honest.count)}`;
}

export function renderFakes(holder: HTMLElement, f: Fakes, link: (l: FakeLink) => string) {
  const card = document.createElement("div");
  card.className = "compare-game";
  const head = document.createElement("h3");
  head.textContent = "Does the trained bot fake?";
  const line = document.createElement("p");
  line.className = "hint";
  line.textContent = fakesLine(f);
  card.append(head, line);
  if (f.links.length > 0) {
    const list = document.createElement("ul");
    list.className = "replays";
    for (const l of f.links) {
      const item = document.createElement("li");
      const a = document.createElement("a");
      a.href = link(l);
      a.textContent = `replay: seed ${l.seed}, ${l.scored ? "scored" : "no goal"}`;
      item.append(a);
      list.append(item);
    }
    card.append(list);
  }
  holder.append(card);
}

export function parseResults(json: unknown): Result[] {
  if (!Array.isArray(json)) {
    return [];
  }
  const out: Result[] = [];
  for (const v of json) {
    const r = record(v);
    if (!r) {
      continue;
    }
    const { game, level, matches, won, lost, mine, theirs } = r;
    if (
      typeof game === "string" &&
      isCount(level) &&
      isCount(matches) &&
      isCount(won) &&
      isCount(lost) &&
      isCount(mine) &&
      isCount(theirs)
    ) {
      out.push({ game, level, matches, won, lost, mine, theirs });
    }
  }
  return out;
}

export const MIN_ATTACKS = 20;

export function attackGain(g: GameRow): number | null {
  const { honest, fake } = g.attacks;
  if (honest.count < MIN_ATTACKS || fake.count < MIN_ATTACKS) {
    return null;
  }
  return fake.scored / fake.count - honest.scored / honest.count;
}

export function verdict(gain: number | null): string {
  if (gain === null) {
    return `deception untested here: fewer than ${MIN_ATTACKS} faked or honest attacks`;
  }
  const signed = Math.round(gain * 100);
  const points = Math.abs(signed);
  if (signed >= 5) {
    return `deception decides: faked attacks score ${points} points more often than honest ones`;
  }
  if (signed <= -5) {
    return `deception costs: faked attacks score ${points} points less often than honest ones`;
  }
  return `deception barely matters here: faked and honest attacks score within ${points} points`;
}

export function attackLine(g: GameRow): string {
  const { honest, fake } = g.attacks;
  if (honest.count + fake.count === 0) {
    return "level 5 made no attacks";
  }
  const pct = (a: number, b: number) => (b === 0 ? "—" : `${Math.round((100 * a) / b)}%`);
  const bites =
    fake.answered === 0
      ? "no fake was answered"
      : `the human model bit on ${pct(fake.bitten, fake.answered)} of the ${fake.answered} fakes it answered`;
  return `level 5 attacks score ${pct(honest.scored, honest.count)} honest (${honest.count}) and ${pct(fake.scored, fake.count)} with a fake (${fake.count}); ${bites}`;
}

export function yours(results: Result[], game: string, level: number): string {
  const r = results.find((x) => x.game === game && x.level === level);
  if (!r || r.matches === 0) {
    return "not played";
  }
  return `you ${r.won}–${r.lost} · ${r.mine}:${r.theirs}`;
}

export function renderComparison(
  holder: HTMLElement,
  comparison: Comparison,
  results: Result[],
  name: (id: string) => string,
) {
  holder.replaceChildren();
  const title = document.createElement("h2");
  title.textContent = "How the same core does in each game";
  const note = document.createElement("p");
  note.className = "hint";
  note.textContent = `Bars: the bot's share of points against a model of a human player, ${comparison.matches} matches per level. Below each bar: your own record.`;
  holder.append(title, note);
  for (const g of comparison.games) {
    const card = document.createElement("div");
    card.className = "compare-game";
    const head = document.createElement("h3");
    head.textContent = name(g.id);
    const bars = document.createElement("div");
    bars.className = "compare-bars";
    for (const l of g.levels) {
      const cell = document.createElement("div");
      cell.className = "compare-cell";
      const bar = document.createElement("div");
      bar.className = "compare-bar";
      bar.style.setProperty("--share", String(l.share));
      const label = document.createElement("b");
      label.textContent = `L${l.level} ${(l.share * 100).toFixed(0)}%`;
      const mine = document.createElement("small");
      mine.textContent = yours(results, g.id, l.level);
      cell.append(bar, label, mine);
      bars.append(cell);
    }
    const five = g.levels[4];
    const line = document.createElement("p");
    line.className = "compare-verdict";
    line.textContent = verdict(attackGain(g));
    const detail = document.createElement("p");
    detail.className = "hint";
    detail.textContent = `match share of level 5: ${(five.share * 100).toFixed(0)}% with fakes (${five.fakes.toFixed(1)} deceptive moves per match), ${(g.honest.share * 100).toFixed(0)}% with fakes and long-game breaks switched off`;
    const attacks = document.createElement("p");
    attacks.className = "hint";
    attacks.textContent = attackLine(g);
    card.append(head, bars, line, detail, attacks);
    holder.append(card);
  }
}
