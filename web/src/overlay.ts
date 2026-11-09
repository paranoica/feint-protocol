export interface Insight {
  intent: number;
  memory: boolean;
  likelyShot: number;
  likelyCover: number[];
  shown: number;
  real: number;
}

const YOURS = ["right", "centre", "left"];
const THEIRS = ["left", "centre", "right"];

export function intentName(intent: number): string {
  if (intent < 0) {
    return "waiting";
  }
  const kind = intent >> 1;
  if (kind === 1) {
    return (intent & 1) === 1 ? "fake" : "strike";
  }
  return kind === 2 ? "defend" : "move";
}

function named(names: string[], c: number): string {
  return c >= 0 && c < names.length ? names[c] : "unsure";
}

export function describe(insight: Insight): string[] {
  const lines = [`bot: ${intentName(insight.intent)}`];
  if (insight.shown >= 0 || insight.real >= 0) {
    const shown = insight.shown < 0 ? "nothing" : named(THEIRS, insight.shown);
    lines.push(`showing ${shown}, playing ${named(THEIRS, insight.real)}`);
  }
  if (!insight.memory) {
    lines.push("model: none at this level");
    return lines;
  }
  lines.push(`your next shot: ${named(YOURS, insight.likelyShot)}`);
  insight.likelyCover.forEach((cover, shown) => {
    lines.push(`bot shows ${named(THEIRS, shown)} → you cover ${named(THEIRS, cover)}`);
  });
  return lines;
}
