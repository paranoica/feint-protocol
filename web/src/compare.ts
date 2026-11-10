export interface LevelRow {
  level: number;
  matches: number;
  won: number;
  mine: number;
  theirs: number;
  share: number;
  fakes: number;
}

export interface GameRow {
  id: string;
  levels: LevelRow[];
  honest: LevelRow;
  payoff: number;
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
    if (levels.length !== 5 || levels.some((l, i) => l === null || l.level !== i + 1) || !honest) {
      return null;
    }
    games.push({ id: row.id, levels: levels as LevelRow[], honest, payoff: row.payoff });
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

export function verdict(payoff: number): string {
  const signed = `${payoff >= 0 ? "+" : "−"}${Math.abs(payoff).toFixed(2)}`;
  if (payoff >= 0.05) {
    return `deception decides: level 5 gains ${payoff.toFixed(2)} of the points by faking`;
  }
  if (payoff <= -0.05) {
    return `deception costs: level 5 loses ${Math.abs(payoff).toFixed(2)} of the points by faking`;
  }
  return `deception barely matters here (${signed})`;
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
    line.textContent = verdict(g.payoff);
    const detail = document.createElement("p");
    detail.className = "hint";
    detail.textContent = `level 5: ${(five.share * 100).toFixed(0)}% with fakes (${five.fakes.toFixed(1)} deceptive moves per match), ${(g.honest.share * 100).toFixed(0)}% with fakes and long-game breaks switched off`;
    card.append(head, bars, line, detail);
    holder.append(card);
  }
}
