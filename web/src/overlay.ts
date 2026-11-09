export interface Insight {
  intent: number;
  memory: boolean;
  likelyShot: number;
  likelyCover: number[];
}

const CLASSES = ["right", "centre", "left"];

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

export function describe(insight: Insight): string[] {
  const lines = [`bot: ${intentName(insight.intent)}`];
  if (!insight.memory) {
    lines.push("model: none at this level");
    return lines;
  }
  const name = (c: number) => (c >= 0 && c < CLASSES.length ? CLASSES[c] : "unsure");
  lines.push(`your next shot: ${name(insight.likelyShot)}`);
  insight.likelyCover.forEach((cover, shown) => {
    lines.push(`shown ${CLASSES[shown] ?? shown} → you cover ${name(cover)}`);
  });
  return lines;
}
