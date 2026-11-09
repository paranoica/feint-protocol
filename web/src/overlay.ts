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

const INTENT_COLORS: Record<string, string> = {
  fake: "#facc15",
  strike: "#ef4444",
  defend: "#a78bfa",
};

export const LEGEND = "colour: yellow fake · red strike · violet defend · grey move";

export function intentColor(intent: number): string {
  return INTENT_COLORS[intentName(intent)] ?? "#94a3b8";
}

export function cornerName(y: number, horizontal: boolean): string {
  if (Math.abs(y) < 0.01) {
    return "middle";
  }
  if (horizontal) {
    return y > 0 ? "top" : "bottom";
  }
  return y > 0 ? "left" : "right";
}

export function describeBot(label: string, insight: Insight, own: string[], other: string[]): string[] {
  const lines = [`${label}: ${intentName(insight.intent)}`];
  if (insight.shown >= 0 || insight.real >= 0) {
    const shown = insight.shown < 0 ? "nothing" : named(own, insight.shown);
    lines.push(`showing ${shown}, playing ${named(own, insight.real)}`);
  }
  if (!insight.memory) {
    lines.push("model: none at this level");
    return lines;
  }
  lines.push(`expects the other's shot: ${named(other, insight.likelyShot)}`);
  insight.likelyCover.forEach((cover, shown) => {
    lines.push(`shows ${named(own, shown)} → other covers ${named(own, cover)}`);
  });
  return lines;
}
