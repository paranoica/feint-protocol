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
const READY = 0.4;
const LEAD = 0.15;
const CORNER = 0.55;
const AIM_RANGE = 0.6;
const CAM_BACK = 2.4;
const CAM_HEIGHT = 1.7;
const BLADE = 0.09;
const PADDLE_Z = 0.25;

export interface Camera {
  f: number;
  cx: number;
  horizon: number;
  camX: number;
}

let layout: Layout = { halfLength: 1.37, halfWidth: 0.7625, reach: 0.32, backRoom: 2, sideRoom: 1 };
let width = 1;
let height = 1;
let camera: Camera = makeCamera(1, 1, layout);
let mouseX = 0;
let fast = false;
let disguise = false;
let colors = { you: "#22d3ee", bot: "#fb7185", ink: "#e7eef8" };

function css(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

export function makeCamera(w: number, h: number, table: Layout): Camera {
  const camX = -(table.halfLength + CAM_BACK);
  const nearZ = CAM_BACK;
  const farZ = CAM_BACK + 2 * table.halfLength + 0.6;
  const byWidth = (0.42 * w * nearZ) / table.halfWidth;
  const byHeight = (0.64 * h) / (CAM_HEIGHT / nearZ - (CAM_HEIGHT - PADDLE_Z) / farZ);
  const f = Math.min(byWidth, byHeight);
  const horizon = 0.74 * h - (CAM_HEIGHT * f) / nearZ;
  return { f, cx: w / 2, horizon, camX };
}

export function project(cam: Camera, x: number, y: number, z: number): [number, number, number] {
  const depth = Math.max(0.3, x - cam.camX);
  const k = cam.f / depth;
  return [cam.cx - y * k, cam.horizon + (CAM_HEIGHT - z) * k, k];
}

export function lateralAt(cam: Camera, sx: number, x: number): number {
  const depth = Math.max(0.3, x - cam.camX);
  return (-(sx - cam.cx) * depth) / cam.f;
}

export function readPaddles(v: number[]): [Paddle, Paddle] {
  const paddle = (base: number): Paddle => ({ x: v[base], y: v[base + 1], facing: v[base + 2] });
  return [paddle(5), paddle(8)];
}

export function autoDepth(v: number[], table: Pick<Layout, "halfLength" | "backRoom">): number {
  const [phase, , bx] = v;
  const hitter = v[11];
  const bounces = v[12];
  const server = v[13];
  if (phase === 0 && server === 0) {
    return v[5];
  }
  if (phase === 1 && hitter === 1 && bounces === 1) {
    return Math.max(-(table.halfLength + table.backRoom), bx - LEAD);
  }
  return -(table.halfLength + READY);
}

export function contactAim(ballY: number, paddleY: number, reach: number): number {
  const offset = Math.max(-1, Math.min(1, (ballY - paddleY) / reach));
  return offset * CORNER;
}

export function shotValues(target: Pair, side: number, quick: boolean, disguised: boolean): number[] {
  const y = AIMS[Math.max(0, Math.min(2, side))];
  const shown = y === 0 ? 0 : Math.sign(y) * SHOWN;
  return [target[0], target[1], 1, DEPTH, y, quick ? FAST : SLOW, disguised ? -shown : shown];
}

export function assistValues(v: number[], side: number, table: Layout, quick: boolean, disguised: boolean, serve: boolean): number[] {
  const [mine] = readPaddles(v);
  const near = Math.abs(v[2] - mine.x) < AIM_RANGE;
  const aim = near ? contactAim(v[3], mine.y, table.reach) : 0;
  const shown = Math.abs(aim) < 0.15 ? 0 : Math.sign(aim) * SHOWN;
  const limit = (x: number, m: number) => Math.max(-m, Math.min(m, x));
  const serving = v[0] === 0 && v[13] === 0;
  return [
    limit(autoDepth(v, table), table.halfLength + table.backRoom),
    limit(side, table.halfWidth + table.sideRoom),
    serving && !serve ? 0 : 1,
    DEPTH,
    aim,
    quick && !serving ? FAST : SLOW,
    disguised ? -shown : shown,
  ];
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

function quad(ctx: CanvasRenderingContext2D, corners: [number, number][]) {
  ctx.beginPath();
  corners.forEach(([x, y], i) => {
    const [sx, sy] = project(camera, x, y, 0);
    if (i === 0) {
      ctx.moveTo(sx, sy);
    } else {
      ctx.lineTo(sx, sy);
    }
  });
  ctx.closePath();
}

function line(ctx: CanvasRenderingContext2D, a: [number, number, number], b: [number, number, number]) {
  const [ax, ay] = project(camera, ...a);
  const [bx, by] = project(camera, ...b);
  ctx.beginPath();
  ctx.moveTo(ax, ay);
  ctx.lineTo(bx, by);
  ctx.stroke();
}

function table(ctx: CanvasRenderingContext2D) {
  const L = layout.halfLength;
  const W = layout.halfWidth;
  ctx.fillStyle = "#14532d";
  quad(ctx, [[-L, -W], [-L, W], [L, W], [L, -W]]);
  ctx.fill();
  ctx.strokeStyle = "rgba(255,255,255,0.85)";
  ctx.lineWidth = 2;
  ctx.stroke();
  line(ctx, [-L, 0, 0], [L, 0, 0]);
  ctx.strokeStyle = "rgba(255,255,255,0.95)";
  ctx.lineWidth = 3;
  line(ctx, [0, W + 0.15, 0.1525], [0, -W - 0.15, 0.1525]);
  ctx.strokeStyle = "rgba(255,255,255,0.35)";
  ctx.lineWidth = 1;
  line(ctx, [0, W + 0.15, 0], [0, W + 0.15, 0.1525]);
  line(ctx, [0, -W - 0.15, 0], [0, -W - 0.15, 0.1525]);
}

function paddle(ctx: CanvasRenderingContext2D, p: Paddle, color: string) {
  const [gx, gy, gk] = project(camera, p.x, p.y, 0);
  ctx.save();
  ctx.strokeStyle = "rgba(255,255,255,0.18)";
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.ellipse(gx, gy, layout.reach * gk, layout.reach * gk * 0.45, 0, 0, Math.PI * 2);
  ctx.stroke();
  ctx.restore();
  const [sx, sy, k] = project(camera, p.x, p.y, PADDLE_Z);
  const r = Math.max(5, BLADE * k);
  ctx.save();
  ctx.shadowColor = color;
  ctx.shadowBlur = r;
  ctx.fillStyle = color;
  ctx.globalAlpha = 0.7;
  ctx.beginPath();
  ctx.ellipse(sx, sy, r, r * 1.1, 0, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
  if (Math.abs(p.facing) > 0.05) {
    const [tx, ty] = project(camera, p.x, p.y + p.facing * 0.35, PADDLE_Z);
    ctx.save();
    ctx.strokeStyle = colors.ink;
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.moveTo(sx, sy);
    ctx.lineTo(tx, ty);
    ctx.stroke();
    ctx.restore();
  }
}

function ball(ctx: CanvasRenderingContext2D, x: number, y: number, z: number) {
  const [gx, gy, k] = project(camera, x, y, 0);
  ctx.fillStyle = "rgba(0,0,0,0.45)";
  ctx.beginPath();
  ctx.ellipse(gx, gy, Math.max(2, 0.03 * k), Math.max(1, 0.015 * k), 0, 0, Math.PI * 2);
  ctx.fill();
  const [bx, by] = project(camera, x, y, z);
  ctx.fillStyle = "#fff7e6";
  ctx.beginPath();
  ctx.arc(bx, by, Math.max(3, 0.025 * k), 0, Math.PI * 2);
  ctx.fill();
}

function banner(ctx: CanvasRenderingContext2D, text: string, color: string) {
  ctx.save();
  ctx.font = `800 ${Math.max(26, height * 0.06)}px system-ui, sans-serif`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.shadowColor = color;
  ctx.shadowBlur = 20;
  ctx.fillStyle = color;
  ctx.fillText(text, width / 2, height * 0.45);
  ctx.restore();
}

export const pingpongView: GameView = {
  id: "pingpong",
  name: "Table tennis",
  pitch: "The paddle shows where you will play. Hit the ball with the paddle's edge to send it to a corner; a disguised shot shows the other side but is slower.",
  hint: "",
  arena: true,
  inputTicks: 2,

  setup(match) {
    const [halfLength, halfWidth, , reach, backRoom, sideRoom] = Array.from(match.layout());
    layout = { halfLength, halfWidth, reach, backRoom, sideRoom };
    camera = makeCamera(width, height, layout);
    mouseX = width / 2;
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
    camera = makeCamera(w, h, layout);
  },

  draw(scene: Scene) {
    const { ctx, match, debug, watching } = scene;
    ctx.clearRect(0, 0, width, height);
    ctx.fillStyle = "#0b1220";
    ctx.fillRect(0, 0, width, height);
    table(ctx);
    const v = Array.from(match.view());
    const phase = v[0];
    const paddles = readPaddles(v);
    const plain = (seat: number) => (seat === 0 ? colors.you : colors.bot);
    const seatColor = (seat: number) => (debug && (watching || seat === 1) ? intentColor(match.intent_of(seat)) : plain(seat));
    paddle(ctx, paddles[1], seatColor(1));
    if (v[2] >= paddles[0].x) {
      ball(ctx, v[2], v[3], v[4]);
      paddle(ctx, paddles[0], seatColor(0));
    } else {
      paddle(ctx, paddles[0], seatColor(0));
      ball(ctx, v[2], v[3], v[4]);
    }
    if (scene.countdown) {
      banner(ctx, scene.countdown, colors.ink);
    } else if (phase === 2 && scene.scorer !== null) {
      const who = scene.scorer;
      banner(ctx, who === 0 ? (watching ? "Near side scores" : "Your point") : watching ? "Far side scores" : "Bot's point", plain(who));
    }
    if (!watching) {
      ctx.fillStyle = disguise ? colors.bot : "rgba(231,238,248,0.5)";
      ctx.font = "600 14px system-ui, sans-serif";
      ctx.textAlign = "left";
      ctx.fillText(disguise ? "disguise ON" : "honest", 16, 56);
    }
    if (!scene.countdown && !watching && phase === 0 && v[13] === 0) {
      ctx.fillStyle = "rgba(231,238,248,0.6)";
      ctx.font = "600 15px system-ui, sans-serif";
      ctx.textAlign = "center";
      ctx.fillText("Mouse moves the paddle sideways · click to serve within 3 s · the paddle's edge sends the ball to a corner · hold for a fast shot · Shift: disguise", width / 2, height - 18);
    }
  },

  debugLines(match, watching: Pair | null) {
    if (!watching) {
      return [null, [LEGEND, ...describe("bot", insightOf(match, 1))]];
    }
    const lines = [0, 1].map((seat) =>
      describe(`${seat === 0 ? "near" : "far"} · level ${watching[seat]}`, insightOf(match, seat)),
    );
    return [[LEGEND, ...lines[0]], lines[1]];
  },

  press(event, rect) {
    this.move(event, rect);
    fast = true;
    return true;
  },

  move(event, rect) {
    mouseX = event.clientX - rect.left;
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
    return false;
  },

  send(match) {
    const v = Array.from(match.view());
    const [mine] = readPaddles(v);
    const side = lateralAt(camera, mouseX, mine.x);
    match.set_input(new Float32Array(assistValues(v, side, layout, fast, disguise, fast)));
  },

  sideColors() {
    return [colors.you, colors.bot];
  },
};
