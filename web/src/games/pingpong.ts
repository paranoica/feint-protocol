import type { WebMatch } from "../wasm/feint_web.js";
import { intentColor, intentName, LEGEND, type Insight } from "../overlay.ts";
import type { GameView, Pair, Scene } from "./types.ts";

interface Layout {
  halfLength: number;
  halfWidth: number;
  reach: number;
  backRoom: number;
  sideRoom: number;
}

export interface Paddle {
  x: number;
  y: number;
  facing: number;
}

const AIMS = [-0.5, 0, 0.5];
const SIDES = ["their left", "the middle", "their right"];
const DEPTH = 1.0;
const SLOW = 0.7;
const FAST = 0.42;
const SHOWN = 0.8;

let layout: Layout = { halfLength: 1.37, halfWidth: 0.7625, reach: 0.32, backRoom: 2, sideRoom: 1 };
let width = 1;
let height = 1;
let scale = 100;
let cursor: Pair = [-1.8, 0];
let aim = 1;
let fast = false;
let disguise = false;
let colors = { you: "#22d3ee", bot: "#fb7185", ink: "#e7eef8" };

function css(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

const toX = (m: number) => width / 2 + m * scale;
const toY = (m: number) => height / 2 - m * scale;

export function readPaddles(v: number[]): [Paddle, Paddle] {
  const paddle = (base: number): Paddle => ({ x: v[base], y: v[base + 1], facing: v[base + 2] });
  return [paddle(5), paddle(8)];
}

export function aimFromKey(key: string, current: number): number {
  switch (key) {
    case "ArrowUp":
    case "w":
    case "W":
      return Math.min(2, current + 1);
    case "ArrowDown":
    case "s":
    case "S":
      return Math.max(0, current - 1);
    default:
      return current;
  }
}

export function shotValues(target: Pair, side: number, quick: boolean, disguised: boolean): number[] {
  const y = AIMS[Math.max(0, Math.min(2, side))];
  const shown = y === 0 ? 0 : Math.sign(y) * SHOWN;
  return [target[0], target[1], 1, DEPTH, y, quick ? FAST : SLOW, disguised ? -shown : shown];
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
  if (!insight.memory) {
    lines.push("model: none at this level");
    return lines;
  }
  const side = insight.likelyShot;
  lines.push(`the other usually plays to ${side >= 0 && side < SIDES.length ? SIDES[side] : "anywhere"}`);
  return lines;
}

function paddle(ctx: CanvasRenderingContext2D, p: Paddle, color: string) {
  const r = Math.max(8, layout.reach * scale * 0.45);
  ctx.save();
  ctx.shadowColor = color;
  ctx.shadowBlur = r;
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.arc(toX(p.x), toY(p.y), r, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
  if (Math.abs(p.facing) > 0.05) {
    ctx.save();
    ctx.strokeStyle = colors.ink;
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.moveTo(toX(p.x), toY(p.y));
    ctx.lineTo(toX(p.x), toY(p.y + p.facing * 0.35));
    ctx.stroke();
    ctx.restore();
  }
  ctx.strokeStyle = "rgba(255,255,255,0.15)";
  ctx.beginPath();
  ctx.arc(toX(p.x), toY(p.y), layout.reach * scale, 0, Math.PI * 2);
  ctx.stroke();
}

function banner(ctx: CanvasRenderingContext2D, text: string, color: string) {
  ctx.save();
  ctx.font = `800 ${Math.max(26, scale * 0.3)}px system-ui, sans-serif`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.shadowColor = color;
  ctx.shadowBlur = 20;
  ctx.fillStyle = color;
  ctx.fillText(text, width / 2, height * 0.12);
  ctx.restore();
}

export const pingpongView: GameView = {
  id: "pingpong",
  name: "Table tennis",
  pitch: "The paddle shows where you will play. Show one corner and hit the other — but a disguised shot is slower.",
  hint: "",
  arena: true,
  inputTicks: 2,

  setup(match) {
    const [halfLength, halfWidth, , reach, backRoom, sideRoom] = Array.from(match.layout());
    layout = { halfLength, halfWidth, reach, backRoom, sideRoom };
    cursor = [-(halfLength + 0.5), 0];
    aim = 1;
    fast = false;
    disguise = false;
    colors = {
      you: css("--you") || colors.you,
      bot: css("--bot") || colors.bot,
      ink: css("--ink") || colors.ink,
    };
  },

  resize(w, h) {
    width = w;
    height = h;
    scale = Math.min(w / (2 * (layout.halfLength + layout.backRoom)), h / (2 * (layout.halfWidth + layout.sideRoom)));
  },

  draw(scene: Scene) {
    const { ctx, match, debug, watching } = scene;
    ctx.clearRect(0, 0, width, height);
    ctx.fillStyle = "#0b1220";
    ctx.fillRect(0, 0, width, height);
    const L = layout.halfLength;
    const W = layout.halfWidth;
    ctx.fillStyle = "#14532d";
    ctx.fillRect(toX(-L), toY(W), 2 * L * scale, 2 * W * scale);
    ctx.strokeStyle = "rgba(255,255,255,0.8)";
    ctx.lineWidth = 2;
    ctx.strokeRect(toX(-L), toY(W), 2 * L * scale, 2 * W * scale);
    ctx.beginPath();
    ctx.moveTo(toX(-L), toY(0));
    ctx.lineTo(toX(L), toY(0));
    ctx.stroke();
    ctx.strokeStyle = "rgba(255,255,255,0.95)";
    ctx.lineWidth = 4;
    ctx.beginPath();
    ctx.moveTo(toX(0), toY(W + 0.15));
    ctx.lineTo(toX(0), toY(-W - 0.15));
    ctx.stroke();
    const v = Array.from(match.view());
    const phase = v[0];
    const [bx, by, bz] = [v[2], v[3], v[4]];
    const paddles = readPaddles(v);
    const plain = (seat: number) => (seat === 0 ? colors.you : colors.bot);
    const seatColor = (seat: number) => (debug && (watching || seat === 1) ? intentColor(match.intent_of(seat)) : plain(seat));
    paddles.forEach((p, seat) => paddle(ctx, p, seatColor(seat)));
    ctx.fillStyle = "rgba(0,0,0,0.45)";
    ctx.beginPath();
    ctx.arc(toX(bx), toY(by), 5, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = "#fff7e6";
    ctx.beginPath();
    ctx.arc(toX(bx), toY(by) - bz * scale * 0.4, Math.max(4, 6 + bz * 6), 0, Math.PI * 2);
    ctx.fill();
    if (scene.countdown) {
      banner(ctx, scene.countdown, colors.ink);
    } else if (phase === 2 && scene.scorer !== null) {
      const who = scene.scorer;
      banner(ctx, who === 0 ? (watching ? "Left scores" : "Your point") : watching ? "Right scores" : "Bot's point", plain(who));
    }
    if (!watching) {
      ctx.fillStyle = disguise ? colors.bot : "rgba(231,238,248,0.5)";
      ctx.font = "600 14px system-ui, sans-serif";
      ctx.textAlign = "left";
      ctx.fillText(`corner: ${["left", "middle", "right"][aim]} · ${disguise ? "disguise ON" : "honest"}`, 16, 24);
    }
    if (!scene.countdown && !watching && phase === 0 && v[13] === 0) {
      ctx.fillStyle = "rgba(231,238,248,0.6)";
      ctx.font = "600 15px system-ui, sans-serif";
      ctx.textAlign = "center";
      ctx.fillText("Move with the mouse · ↑ / ↓ pick the corner · click to serve within 3 s · hold in a rally for a fast shot · Shift turns the disguise on or off", width / 2, height - 18);
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
    fast = true;
    return true;
  },

  move(event, rect) {
    cursor = [(event.clientX - rect.left - width / 2) / scale, -(event.clientY - rect.top - height / 2) / scale];
  },

  release() {
    fast = false;
    return true;
  },

  key(event) {
    if (event.key === "Shift") {
      if (!event.repeat) {
        disguise = !disguise;
      }
      return true;
    }
    const next = aimFromKey(event.key, aim);
    if (next !== aim) {
      aim = next;
      return true;
    }
    return false;
  },

  send(match) {
    const limit = (v: number, m: number) => Math.max(-m, Math.min(m, v));
    const target: Pair = [limit(cursor[0], layout.halfLength + layout.backRoom), limit(cursor[1], layout.halfWidth + layout.sideRoom)];
    const v = match.view();
    const serving = v[0] === 0 && v[13] === 0;
    const values = shotValues(target, aim, fast && !serving, disguise);
    values[2] = serving && !fast ? 0 : 1;
    match.set_input(new Float32Array(values));
  },

  sideColors() {
    return [colors.you, colors.bot];
  },
};
