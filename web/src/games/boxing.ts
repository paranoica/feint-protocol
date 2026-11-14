import type { WebMatch } from "../wasm/feint_web.js";
import { intentColor, intentName, LEGEND, type Insight } from "../overlay.ts";
import type { GameView, Pair, Scene } from "./types.ts";

interface Layout {
  ring: number;
  startGap: number;
  minGap: number;
  jabReach: number;
  hookReach: number;
}

export interface Fighter {
  x: number;
  stage: number;
  kind: number;
  height: number;
  left: number;
  guard: number;
  covering: boolean;
}

interface Pending {
  kind: number;
  height: number;
  until: number;
}

const HEIGHTS = ["low", "high"];
const WINDUP = 1;
const STRIKE = 2;
const RECOVER = 3;
const BUFFER_MS = 400;
const LIMB_HIGH = 0.75;
const LIMB_LOW = 0.35;

let layout: Layout = { ring: 3, startGap: 1.6, minGap: 0.4, jabReach: 0.85, hookReach: 0.7 };
let width = 1;
let height = 1;
let scale = 100;
let cursor = -0.8;
let guard = 1;
let pending: Pending | null = null;
let cancel = false;
let colors = { you: "#22d3ee", bot: "#fb7185", ink: "#e7eef8" };

function css(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

const toX = (m: number) => width / 2 + m * scale;
const ground = () => height * 0.75;
const toY = (m: number) => ground() - m * scale;

export function readFighters(v: number[]): [Fighter, Fighter] {
  const fighter = (base: number): Fighter => ({
    x: v[base],
    stage: v[base + 1],
    kind: v[base + 2],
    height: v[base + 3],
    left: v[base + 4],
    guard: v[base + 5],
    covering: v[base + 6] >= 0.5,
  });
  return [fighter(2), fighter(9)];
}

export function attackFromKey(key: string): [number, number] | null {
  switch (key) {
    case "j":
    case "J":
      return [1, 1];
    case "n":
    case "N":
      return [1, -1];
    case "k":
    case "K":
      return [2, 1];
    case "m":
    case "M":
      return [2, -1];
    default:
      return null;
  }
}

export function guardFromKey(key: string, current: number): number {
  switch (key) {
    case "ArrowUp":
    case "w":
    case "W":
      return 1;
    case "ArrowDown":
    case "s":
    case "S":
      return -1;
    default:
      return current;
  }
}

export function boxingValues(target: number, guardHeight: number, attack: [number, number] | null, cancelling: boolean): number[] {
  const [kind, h] = attack ?? [0, 0];
  return [target, guardHeight, kind, h, cancelling ? 1 : 0];
}

function insightOf(match: WebMatch, seat: number): Insight {
  return {
    intent: match.intent_of(seat),
    memory: match.memory_of(seat),
    likelyShot: match.likely_shot_of(seat),
    likelyCover: Array.from(match.likely_cover_of(seat)),
    shown: match.shown_of(seat),
    real: match.real_of(seat),
  };
}

function describe(label: string, insight: Insight): string[] {
  const lines = [`${label}: ${intentName(insight.intent)}`];
  if (insight.shown >= 0 || insight.real >= 0) {
    const name = (c: number) => (c >= 0 && c < HEIGHTS.length ? HEIGHTS[c] : "nothing");
    lines.push(`shows ${name(insight.shown)}, means ${name(insight.real)}`);
  }
  if (!insight.memory) {
    lines.push("model: none at this level");
    return lines;
  }
  const side = insight.likelyShot;
  lines.push(`the other usually punches ${side >= 0 && side < HEIGHTS.length ? HEIGHTS[side] : "anywhere"}`);
  insight.likelyCover.forEach((cover, shown) => {
    if (cover >= 0 && cover < HEIGHTS.length) {
      lines.push(`shown ${HEIGHTS[shown]} → guards ${HEIGHTS[cover]}`);
    }
  });
  return lines;
}

function figure(ctx: CanvasRenderingContext2D, f: Fighter, facing: number, color: string) {
  const x = toX(f.x);
  const head = Math.max(6, 0.12 * scale);
  ctx.save();
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = Math.max(3, 0.05 * scale);
  ctx.lineCap = "round";
  ctx.globalAlpha = f.stage === RECOVER ? 0.55 : 1;
  ctx.beginPath();
  ctx.moveTo(x, toY(0));
  ctx.lineTo(x, toY(0.95));
  ctx.stroke();
  ctx.beginPath();
  ctx.arc(x, toY(1.08), head, 0, Math.PI * 2);
  ctx.fill();
  const guardY = toY(f.guard > 0 ? LIMB_HIGH : LIMB_LOW);
  ctx.globalAlpha = f.covering ? 1 : 0.3;
  ctx.setLineDash(f.covering ? [] : [4, 4]);
  ctx.beginPath();
  ctx.moveTo(x + facing * 0.12 * scale, guardY - 0.12 * scale);
  ctx.lineTo(x + facing * 0.12 * scale, guardY + 0.12 * scale);
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.globalAlpha = 1;
  if (f.stage === WINDUP || f.stage === STRIKE) {
    const y = toY(f.height > 0 ? LIMB_HIGH : LIMB_LOW);
    const reach = f.kind >= 2 ? layout.hookReach : layout.jabReach;
    const out = f.stage === STRIKE ? reach : -0.2;
    ctx.strokeStyle = f.kind >= 2 ? colors.ink : color;
    ctx.lineWidth = Math.max(4, (f.kind >= 2 ? 0.08 : 0.05) * scale);
    ctx.beginPath();
    ctx.moveTo(x, toY(0.8));
    ctx.lineTo(x + facing * out * scale, y);
    ctx.stroke();
    if (f.stage === WINDUP) {
      ctx.font = `700 ${Math.max(12, 0.12 * scale)}px system-ui, sans-serif`;
      ctx.textAlign = "center";
      ctx.fillText(`${f.kind >= 2 ? "hook" : "jab"} ${f.height > 0 ? "high" : "low"}`, x, toY(1.35));
    }
  }
  ctx.restore();
}

function banner(ctx: CanvasRenderingContext2D, text: string, color: string) {
  ctx.save();
  ctx.font = `800 ${Math.max(26, scale * 0.3)}px system-ui, sans-serif`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.shadowColor = color;
  ctx.shadowBlur = 20;
  ctx.fillStyle = color;
  ctx.fillText(text, width / 2, height * 0.15);
  ctx.restore();
}

export const boxingView: GameView = {
  id: "boxing",
  name: "Boxing",
  pitch: "A wind-up shows where the punch goes. Show a hook, let the guard follow, cancel and jab the other height.",
  hint: "",
  arena: true,
  inputTicks: 2,

  setup(match) {
    const [ring, startGap, minGap, jabReach, hookReach] = Array.from(match.layout());
    layout = { ring, startGap, minGap, jabReach, hookReach };
    cursor = -startGap / 2;
    guard = 1;
    pending = null;
    cancel = false;
    colors = {
      you: css("--you") || colors.you,
      bot: css("--bot") || colors.bot,
      ink: css("--ink") || colors.ink,
    };
  },

  resize(w, h) {
    width = w;
    height = h;
    scale = Math.min(w / (2 * (layout.ring + 0.3)), h / 2.2);
  },

  draw(scene: Scene) {
    const { ctx, match, debug, watching } = scene;
    ctx.clearRect(0, 0, width, height);
    ctx.fillStyle = "#0b1220";
    ctx.fillRect(0, 0, width, height);
    ctx.strokeStyle = "rgba(255,255,255,0.6)";
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.moveTo(toX(-layout.ring), toY(0));
    ctx.lineTo(toX(layout.ring), toY(0));
    ctx.stroke();
    for (const edge of [-layout.ring, layout.ring]) {
      ctx.beginPath();
      ctx.moveTo(toX(edge), toY(0));
      ctx.lineTo(toX(edge), toY(1.2));
      ctx.stroke();
    }
    const v = Array.from(match.view());
    const phase = v[0];
    const fighters = readFighters(v);
    const plain = (seat: number) => (seat === 0 ? colors.you : colors.bot);
    const seatColor = (seat: number) => (debug && (watching || seat === 1) ? intentColor(match.intent_of(seat)) : plain(seat));
    fighters.forEach((f, seat) => figure(ctx, f, seat === 0 ? 1 : -1, seatColor(seat)));
    if (scene.countdown) {
      banner(ctx, scene.countdown, colors.ink);
    } else if (phase === 1 && scene.scorer !== null) {
      const who = scene.scorer;
      banner(ctx, who === 0 ? (watching ? "Left lands" : "You land") : watching ? "Right lands" : "Bot lands", plain(who));
    }
    if (!watching) {
      ctx.fillStyle = "rgba(231,238,248,0.5)";
      ctx.font = "600 14px system-ui, sans-serif";
      ctx.textAlign = "left";
      ctx.fillText(`guard: ${guard > 0 ? "high" : "low"}`, 16, 24);
      if (!scene.countdown && phase === 0 && match.tick() < 240 * 6) {
        ctx.fillStyle = "rgba(231,238,248,0.6)";
        ctx.font = "600 15px system-ui, sans-serif";
        ctx.textAlign = "center";
        ctx.fillText("Mouse moves · W / S guard high or low · J / N jab high or low · K / M hook high or low · Space cancels a wind-up", width / 2, height - 18);
      }
    }
  },

  debugLines(match, watching: Pair | null) {
    if (!watching) {
      return [null, [LEGEND, ...describe("bot", insightOf(match, 1))]];
    }
    const lines = [0, 1].map((seat) =>
      describe(`${seat === 0 ? "left" : "right"} · level ${watching[seat]}`, insightOf(match, seat)),
    );
    return [[LEGEND, ...lines[0]], lines[1]];
  },

  press(event, rect) {
    this.move(event, rect);
    return true;
  },

  move(event, rect) {
    cursor = Math.max(-layout.ring, Math.min(layout.ring, (event.clientX - rect.left - width / 2) / scale));
  },

  release() {
    return true;
  },

  key(event) {
    if (event.key === " ") {
      cancel = true;
      return true;
    }
    const attack = attackFromKey(event.key);
    if (attack) {
      if (!event.repeat) {
        pending = { kind: attack[0], height: attack[1], until: performance.now() + BUFFER_MS };
      }
      return true;
    }
    const next = guardFromKey(event.key, guard);
    if (next !== guard) {
      guard = next;
      return true;
    }
    return false;
  },

  send(match) {
    const mine = readFighters(Array.from(match.view()))[0];
    const started = mine.stage === WINDUP && pending !== null && mine.kind === pending.kind && mine.height === pending.height;
    if (pending && (started || performance.now() > pending.until)) {
      pending = null;
    }
    const attack: [number, number] | null = pending ? [pending.kind, pending.height] : null;
    match.set_input(new Float32Array(boxingValues(cursor, guard, attack, false)));
    if (cancel) {
      match.push_input(new Float32Array(boxingValues(cursor, guard, null, true)));
      cancel = false;
    }
  },

  sideColors() {
    return [colors.you, colors.bot];
  },
};
