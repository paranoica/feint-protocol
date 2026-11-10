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

export function attackGain(g: GameRow): number | null {
  const { honest, fake } = g.attacks;
  if (honest.count === 0 || fake.count === 0) {
    return null;
  }
  return fake.scored / fake.count - honest.scored / honest.count;
}

export function verdict(gain: number | null): string {
  if (gain === null) {
    return "deception untested here: level 5 made no faked or no honest attacks";
  }
  const points = Math.round(Math.abs(gain) * 100);
  if (gain >= 0.05) {
    return `deception decides: faked attacks score ${points} points more often than honest ones`;
  }
  if (gain <= -0.05) {
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
  return `level 5 attacks score ${pct(honest.scored, honest.count)} honest (${honest.count}) and ${pct(fake.scored, fake.count)} with a fake (${fake.count}); the human model bit on ${pct(fake.bitten, fake.answered)} of fakes`;
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
