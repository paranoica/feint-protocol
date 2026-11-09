import type { WebMatch } from "../wasm/feint_web.js";
import { intentColor, intentName, LEGEND, type Insight } from "../overlay.ts";
import type { GameView, Pair, Scene } from "./types.ts";

interface Layout {
  halfWidth: number;
  hitRadius: number;
  step: number;
  commitAt: number;
}

interface Gunner {
  x: number;
  hand: number;
  committed: boolean;
  loaded: boolean;
  recovery: number;
}

const SHOWN = ["a fake", "a draw"];
const RESPONSES = ["steps left early", "stands", "steps right early", "steps left late", "steps right late"];
const LEADS = ["leads left", "aims straight", "leads right"];

let layout: Layout = { halfWidth: 2.5, hitRadius: 0.3, step: 1, commitAt: 0.7 };
let width = 1;
let height = 1;
let scale = 60;
let cursor = 0;
let holding = false;
let pendingStep = 0;
let colors = { you: "#22d3ee", bot: "#fb7185", ink: "#e7eef8" };

function css(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

const centreX = () => width / 2;
const rowY = (seat: number) => (seat === 0 ? height * 0.74 : height * 0.26);
const toX = (meters: number) => centreX() + meters * scale;

export function readGunners(v: number[]): [Gunner, Gunner] {
  const gunner = (base: number): Gunner => ({
    x: v[base],
    hand: v[base + 1],
    committed: v[base + 2] >= 0.5,
    loaded: v[base + 3] >= 0.5,
    recovery: v[base + 4],
  });
  return [gunner(2), gunner(7)];
}

export function stepFromKey(key: string): number {
  switch (key) {
    case "q":
    case "Q":
    case "ArrowLeft":
      return -1;
    case "e":
    case "E":
    case "ArrowRight":
      return 1;
    default:
      return 0;
  }
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

function named(names: string[], i: number): string {
  return i >= 0 && i < names.length ? names[i] : "unsure";
}

export function describeDuelist(label: string, insight: Insight): string[] {
  const lines = [`${label}: ${intentName(insight.intent)}`];
  if (!insight.memory) {
    lines.push("model: none at this level");
    return lines;
  }
  lines.push(`the other usually ${named(LEADS, insight.likelyShot)}`);
  insight.likelyCover.forEach((response, shown) => {
    if (shown < SHOWN.length) {
      lines.push(`on ${SHOWN[shown]} the other ${named(RESPONSES, response)}`);
    }
  });
  return lines;
}

function figure(ctx: CanvasRenderingContext2D, g: Gunner, seat: number, color: string, commitAt: number) {
  const x = toX(g.x);
  const y = rowY(seat);
  const size = Math.max(14, scale * 0.32);
  ctx.save();
  ctx.globalAlpha = g.recovery > 0 ? 0.55 : 1;
  ctx.shadowColor = color;
  ctx.shadowBlur = size * 0.8;
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.roundRect(x - size * 0.45, y - size * 0.9, size * 0.9, size * 1.8, size * 0.35);
  ctx.fill();
  ctx.restore();
  const facing = seat === 0 ? -1 : 1;
  const reach = size * 1.5;
  const turn = Math.PI * g.hand;
  const ax = x + size * 0.55;
  ctx.save();
  ctx.strokeStyle = g.committed ? "#ef4444" : colors.ink;
  ctx.lineWidth = Math.max(3, size * 0.18);
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.moveTo(ax, y);
  ctx.lineTo(ax + Math.sin(turn) * reach * 0.6, y - facing * Math.cos(turn) * reach);
  ctx.stroke();
  ctx.restore();
  const barW = size * 2.2;
  const barY = y + (seat === 0 ? size * 1.4 : -size * 1.4);
  ctx.fillStyle = "rgba(255,255,255,0.12)";
  ctx.fillRect(x - barW / 2, barY - 3, barW, 6);
  ctx.fillStyle = g.committed ? "#ef4444" : color;
  ctx.fillRect(x - barW / 2, barY - 3, barW * g.hand, 6);
  ctx.fillStyle = colors.ink;
  ctx.fillRect(x - barW / 2 + barW * commitAt - 1, barY - 6, 2, 12);
  if (!g.loaded) {
    ctx.fillStyle = "rgba(255,255,255,0.45)";
    ctx.font = `600 ${Math.max(11, size * 0.45)}px system-ui, sans-serif`;
    ctx.textAlign = "center";
    ctx.fillText("reloading", x, barY + (seat === 0 ? 24 : -14));
  }
}

function banner(ctx: CanvasRenderingContext2D, text: string, color: string) {
  ctx.save();
  ctx.font = `800 ${Math.max(28, scale * 0.7)}px system-ui, sans-serif`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.shadowColor = color;
  ctx.shadowBlur = 24;
  ctx.fillStyle = color;
  ctx.fillText(text, centreX(), height * 0.5);
  ctx.restore();
}

export const duelView: GameView = {
  id: "duel",
  name: "Quick-draw duel",
  pitch: "Hold to draw, release to fake. Past the mark the shot is locked. Step aside before the bullet lands.",
  hint: "",
  arena: true,
  inputTicks: 2,

  setup(match) {
    const [halfWidth, hitRadius, step, commitAt] = Array.from(match.layout());
    layout = { halfWidth, hitRadius, step, commitAt };
    cursor = 0;
    holding = false;
    pendingStep = 0;
    colors = {
      you: css("--you") || colors.you,
      bot: css("--bot") || colors.bot,
      ink: css("--ink") || colors.ink,
    };
  },

  resize(w, h) {
    width = w;
    height = h;
    scale = Math.min(w / (2 * layout.halfWidth + 1.5), h / 5);
  },

  draw(scene: Scene) {
    const { ctx, match, debug, watching } = scene;
    ctx.clearRect(0, 0, width, height);
    const sand = ctx.createLinearGradient(0, 0, 0, height);
    sand.addColorStop(0, "#1b130b");
    sand.addColorStop(1, "#0d0906");
    ctx.fillStyle = sand;
    ctx.fillRect(0, 0, width, height);
    const v = Array.from(match.view());
    const phase = v[0];
    const gunners = readGunners(v);
    const plain = (seat: number) => (seat === 0 ? colors.you : colors.bot);
    const seatColor = (seat: number) => (debug && (watching || seat === 1) ? intentColor(match.intent_of(seat)) : plain(seat));
    ctx.strokeStyle = "rgba(255,220,170,0.18)";
    ctx.lineWidth = 1;
    for (const seat of [0, 1]) {
      ctx.beginPath();
      ctx.moveTo(toX(-layout.halfWidth), rowY(seat));
      ctx.lineTo(toX(layout.halfWidth), rowY(seat));
      ctx.stroke();
      for (let m = -Math.floor(layout.halfWidth); m <= layout.halfWidth; m += layout.step) {
        ctx.fillStyle = "rgba(255,220,170,0.25)";
        ctx.fillRect(toX(m) - 1, rowY(seat) - 4, 2, 8);
      }
    }
    if (!watching) {
      ctx.save();
      ctx.strokeStyle = "rgba(255,255,255,0.35)";
      ctx.setLineDash([6, 6]);
      ctx.beginPath();
      ctx.moveTo(toX(cursor), rowY(1) - scale * 0.6);
      ctx.lineTo(toX(cursor), rowY(1) + scale * 0.6);
      ctx.stroke();
      ctx.restore();
    }
    for (const seat of [0, 1]) {
      const flying = v[12 + 3 * seat] >= 0.5;
      if (!flying) {
        continue;
      }
      const bx = toX(v[13 + 3 * seat]);
      const progress = v[14 + 3 * seat];
      const from = rowY(seat);
      const to = rowY(1 - seat);
      const y = from + (to - from) * progress;
      ctx.save();
      ctx.strokeStyle = "rgba(255,235,180,0.9)";
      ctx.shadowColor = "#ffd27a";
      ctx.shadowBlur = 12;
      ctx.lineWidth = 3;
      ctx.beginPath();
      ctx.moveTo(bx, y - (to - from) * 0.06);
      ctx.lineTo(bx, y);
      ctx.stroke();
      ctx.restore();
    }
    for (const seat of [0, 1]) {
      figure(ctx, gunners[seat], seat, seatColor(seat), layout.commitAt);
    }
    const last = v[18];
    if (phase === 2 && last >= 0) {
      if (last === 2) {
        banner(ctx, "Draw — both missed", colors.ink);
      } else {
        banner(ctx, last === 0 ? (watching ? "Left hits" : "You hit!") : watching ? "Right hits" : "Hit", plain(last));
      }
    } else if (scene.countdown) {
      banner(ctx, scene.countdown, colors.ink);
    } else if (phase === 0) {
      banner(ctx, "Ready…", colors.ink);
    } else if (!watching && phase === 1) {
      ctx.fillStyle = "rgba(231,238,248,0.55)";
      ctx.font = "600 15px system-ui, sans-serif";
      ctx.textAlign = "center";
      ctx.fillText("Hold the mouse to draw, aim with it · Q / E or ← / → to step aside", centreX(), height * 0.5);
    }
  },

  debugLines(match, watching: Pair | null) {
    if (!watching) {
      return [null, [LEGEND, ...describeDuelist("bot", insightOf(match, 1))]];
    }
    const lines = [0, 1].map((seat) =>
      describeDuelist(`${seat === 0 ? "left" : "right"} · level ${watching[seat]}`, insightOf(match, seat)),
    );
    return [[LEGEND, ...lines[0]], lines[1]];
  },

  press(event, rect) {
    cursor = Math.max(-layout.halfWidth, Math.min(layout.halfWidth, (event.clientX - rect.left - centreX()) / scale));
    holding = true;
    return true;
  },

  move(event, rect) {
    cursor = Math.max(-layout.halfWidth, Math.min(layout.halfWidth, (event.clientX - rect.left - centreX()) / scale));
  },

  release() {
    holding = false;
    return true;
  },

  key(event) {
    const step = stepFromKey(event.key);
    if (step !== 0) {
      pendingStep = step;
      return true;
    }
    return false;
  },

  send(match) {
    const hold = holding ? 1 : 0;
    match.set_input(new Float32Array([cursor, hold, 0]));
    if (pendingStep !== 0) {
      match.push_input(new Float32Array([cursor, hold, pendingStep]));
      pendingStep = 0;
    }
  },

  sideColors() {
    return [colors.you, colors.bot];
  },
};
