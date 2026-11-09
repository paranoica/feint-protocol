import type { WebMatch } from "../wasm/feint_web.js";
import { idle, press, release, steers, type Drag } from "../drag.ts";
import { fit, toCanvas, toTable, TABLE, type Fit, type Table } from "../geometry.ts";
import { follow, goalBanner, shade } from "../hud.ts";
import { cornerName, describe, describeBot, intentColor, LEGEND, type Insight } from "../overlay.ts";
import type { GameView, Pair, Scene } from "./types.ts";

const PAD = 30;
const TRAIL = 10;
const HOLE_SPACING = 0.05;

interface Colors {
  you: string;
  bot: string;
  surface: string;
  surfaceDeep: string;
  rail: string;
  railLight: string;
  line: string;
  hole: string;
  ink: string;
}

function css(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

function readColors(): Colors {
  return {
    you: css("--you"),
    bot: css("--bot"),
    surface: css("--surface"),
    surfaceDeep: css("--surface-deep"),
    rail: css("--rail"),
    railLight: css("--rail-light"),
    line: css("--line"),
    hole: css("--hole"),
    ink: css("--ink"),
  };
}

let view: Fit = fit({ width: 1, height: 1, dpr: 1 });
let dpr = 1;
let table: Table = TABLE;
let sizes = { mouth: 0.15, puck: 0.05, mallet: 0.07 };
let colors = readColors();
let surface: HTMLCanvasElement | null = null;
let pointer: [number, number] = [-0.8, 0];
let drag: Drag = idle;
let trail: Array<[number, number]> = [];

function at(x: number, y: number): [number, number] {
  return toCanvas(view, x, y, table);
}

function tableRect(f: Fit): [number, number, number, number] {
  const [ax, ay] = toCanvas(f, -table.halfLength, table.halfWidth, table);
  const [bx, by] = toCanvas(f, table.halfLength, -table.halfWidth, table);
  return [Math.min(ax, bx), Math.min(ay, by), Math.abs(bx - ax), Math.abs(by - ay)];
}

function paintSurface(width: number, height: number): HTMLCanvasElement {
  const layer = document.createElement("canvas");
  layer.width = Math.round(width * dpr);
  layer.height = Math.round(height * dpr);
  const g = layer.getContext("2d") as CanvasRenderingContext2D;
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  const f = view;
  const [x, y, w, h] = tableRect(f);
  const rail = Math.max(10, PAD * 0.7);
  g.fillStyle = colors.rail;
  g.beginPath();
  g.roundRect(x - rail, y - rail, w + 2 * rail, h + 2 * rail, rail + 10);
  g.fill();
  g.strokeStyle = colors.railLight;
  g.lineWidth = 2;
  g.stroke();
  const fill = g.createLinearGradient(x, y, x + w, y + h);
  fill.addColorStop(0, colors.surface);
  fill.addColorStop(1, colors.surfaceDeep);
  g.fillStyle = fill;
  g.beginPath();
  g.roundRect(x, y, w, h, 12);
  g.fill();
  g.save();
  g.clip();
  g.fillStyle = colors.hole;
  const step = HOLE_SPACING * f.scale;
  const dot = Math.max(1, f.scale * 0.004);
  for (let hx = x + step / 2; hx < x + w; hx += step) {
    for (let hy = y + step / 2; hy < y + h; hy += step) {
      g.fillRect(hx - dot / 2, hy - dot / 2, dot, dot);
    }
  }
  g.strokeStyle = colors.line;
  g.lineWidth = 2;
  const [c1x, c1y] = toCanvas(f, 0, table.halfWidth, table);
  const [c2x, c2y] = toCanvas(f, 0, -table.halfWidth, table);
  g.beginPath();
  g.moveTo(c1x, c1y);
  g.lineTo(c2x, c2y);
  g.stroke();
  const [cx, cy] = toCanvas(f, 0, 0, table);
  g.beginPath();
  g.arc(cx, cy, 0.15 * f.scale, 0, Math.PI * 2);
  g.stroke();
  for (const end of [-1, 1]) {
    const [gx, gy] = toCanvas(f, end * table.halfLength, 0, table);
    g.beginPath();
    g.arc(gx, gy, 0.25 * f.scale, 0, Math.PI * 2);
    g.stroke();
  }
  g.restore();
  for (const [end, color] of [
    [-1, colors.you],
    [1, colors.bot],
  ] as const) {
    const [ax, ay] = toCanvas(f, end * table.halfLength, sizes.mouth, table);
    const [bx, by] = toCanvas(f, end * table.halfLength, -sizes.mouth, table);
    g.save();
    g.strokeStyle = color;
    g.shadowColor = color;
    g.shadowBlur = 18;
    g.lineWidth = Math.max(6, rail * 0.55);
    g.lineCap = "round";
    g.beginPath();
    g.moveTo(ax, ay);
    g.lineTo(bx, by);
    g.stroke();
    g.restore();
  }
  return layer;
}

function disc(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  r: number,
  inner: string,
  outer: string,
  glow: string,
) {
  const [cx, cy] = at(x, y);
  const radius = r * view.scale;
  ctx.save();
  ctx.shadowColor = glow;
  ctx.shadowBlur = radius * 0.8;
  const fill = ctx.createRadialGradient(cx - radius * 0.3, cy - radius * 0.3, radius * 0.1, cx, cy, radius);
  fill.addColorStop(0, inner);
  fill.addColorStop(1, outer);
  ctx.fillStyle = fill;
  ctx.beginPath();
  ctx.arc(cx, cy, radius, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
  return [cx, cy, radius] as const;
}

function mallet(ctx: CanvasRenderingContext2D, x: number, y: number, color: string) {
  const [cx, cy, radius] = disc(ctx, x, y, sizes.mallet, color, shade(color, 0.55), color);
  ctx.strokeStyle = "rgba(255,255,255,0.35)";
  ctx.lineWidth = Math.max(1.5, radius * 0.08);
  ctx.beginPath();
  ctx.arc(cx, cy, radius * 0.72, 0, Math.PI * 2);
  ctx.stroke();
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.arc(cx, cy, radius * 0.42, 0, Math.PI * 2);
  ctx.fill();
}

function puck(ctx: CanvasRenderingContext2D, x: number, y: number) {
  trail = follow(trail, [x, y], TRAIL, 0.45);
  const radius = sizes.puck * view.scale;
  ctx.save();
  ctx.lineCap = "round";
  for (let i = 1; i < trail.length; i++) {
    const [ax, ay] = at(trail[i - 1][0], trail[i - 1][1]);
    const [bx, by] = at(trail[i][0], trail[i][1]);
    const k = i / trail.length;
    ctx.strokeStyle = `rgba(200, 225, 255, ${0.16 * k})`;
    ctx.lineWidth = radius * 1.6 * k;
    ctx.beginPath();
    ctx.moveTo(ax, ay);
    ctx.lineTo(bx, by);
    ctx.stroke();
  }
  ctx.restore();
  const [cx, cy] = disc(ctx, x, y, sizes.puck, "#ffffff", "#c9d3e3", "rgba(255,255,255,0.6)");
  ctx.strokeStyle = "rgba(10,20,40,0.25)";
  ctx.lineWidth = Math.max(1, radius * 0.12);
  ctx.beginPath();
  ctx.arc(cx, cy, radius * 0.6, 0, Math.PI * 2);
  ctx.stroke();
}

export function centreText(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  text: string,
  color: string,
  size: number,
) {
  ctx.save();
  ctx.font = `800 ${size}px system-ui, sans-serif`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.shadowColor = color;
  ctx.shadowBlur = 24;
  ctx.fillStyle = color;
  ctx.fillText(text, cx, cy);
  ctx.restore();
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

function cornerNames(match: WebMatch, attacker: number): string[] {
  return [0, 1, 2].map((k) => cornerName(match.class_point(attacker, k)[1], view.horizontal));
}

function mark(
  ctx: CanvasRenderingContext2D,
  match: WebMatch,
  attacker: number,
  cls: number,
  color: string,
  filled: boolean,
) {
  if (cls < 0) {
    return;
  }
  const [x, y] = Array.from(match.class_point(attacker, cls));
  const [cx, cy] = at(x, y);
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = 2;
  if (filled) {
    ctx.fillRect(cx - 6, cy - 6, 12, 12);
  } else {
    ctx.strokeRect(cx - 8, cy - 8, 16, 16);
  }
}

function aimAt(event: PointerEvent, rect: DOMRect) {
  pointer = toTable(view, event.clientX - rect.left, event.clientY - rect.top, table);
}

export const hockey: GameView = {
  id: "airhockey",
  name: "Air hockey",
  pitch: "Drag your mallet, beat the goalie. Fakes, reads and a long game.",
  hint: "Hold and drag your mallet",
  arena: true,
  inputTicks: 4,

  setup(match) {
    const [halfLength, halfWidth, mouth, puckR, malletR] = Array.from(match.layout());
    table = { halfLength, halfWidth };
    sizes = { mouth, puck: puckR, mallet: malletR };
    pointer = [-0.8, 0];
    drag = idle;
    trail = [];
  },

  resize(width, height, ratio) {
    dpr = ratio;
    view = fit({ width, height, dpr }, table, Math.min(PAD + 40, 24 + 0.05 * Math.min(width, height)));
    colors = readColors();
    surface = paintSurface(width, height);
  },

  draw(scene: Scene) {
    const { ctx, match, debug, watching } = scene;
    ctx.clearRect(0, 0, scene.width, scene.height);
    if (surface) {
      ctx.drawImage(surface, 0, 0, scene.width, scene.height);
    }
    const p = match.view();
    mallet(ctx, p[4], p[5], debug && watching ? intentColor(match.intent_of(0)) : colors.you);
    mallet(ctx, p[8], p[9], debug ? intentColor(match.intent_of(1)) : colors.bot);
    puck(ctx, p[0], p[1]);
    if (scene.pauseFull > 0 && match.serve_pause() > 0 && scene.scorer !== null) {
      trail = [];
    }
    const big = Math.max(24, view.scale * 0.08);
    const [cx, cy] = at(0, 0);
    const banner = goalBanner(match.serve_pause(), scene.pauseFull, scene.scorer);
    if (banner) {
      const color = banner.side === 0 ? colors.you : colors.bot;
      const text = watching ? `Level ${watching[banner.side]} scores` : banner.text;
      centreText(ctx, cx, cy, text, color, big);
      const width = view.scale * 0.5;
      ctx.fillStyle = "rgba(255,255,255,0.12)";
      ctx.fillRect(cx - width / 2, cy + big * 0.7, width, 4);
      ctx.fillStyle = color;
      ctx.fillRect(cx - width / 2, cy + big * 0.7, width * banner.progress, 4);
    }
    if (scene.countdown) {
      centreText(ctx, cx, cy, scene.countdown, colors.ink, big * 2.8);
    }
    if (!debug) {
      return;
    }
    if (watching) {
      for (const seat of [0, 1]) {
        const insight = insightOf(match, seat);
        const color = intentColor(insight.intent);
        mark(ctx, match, seat, insight.shown, color, false);
        mark(ctx, match, seat, insight.real, color, true);
      }
      return;
    }
    const insight = insightOf(match, 1);
    mark(ctx, match, 0, insightOf(match, 1).likelyShot, colors.you, false);
    mark(ctx, match, 1, insight.shown, colors.bot, false);
    mark(ctx, match, 1, insight.real, colors.bot, true);
  },

  debugLines(match, watching: Pair | null) {
    if (!watching) {
      return [null, [LEGEND, ...describe(insightOf(match, 1))]];
    }
    const lines = [0, 1].map((seat) =>
      describeBot(
        `${seat === 0 ? "left" : "right"} · level ${watching[seat]}`,
        insightOf(match, seat),
        cornerNames(match, seat),
        cornerNames(match, 1 - seat),
      ),
    );
    return [[LEGEND, ...lines[0]], lines[1]];
  },

  press(event, rect) {
    const primary = event.isPrimary && (event.pointerType !== "mouse" || event.button === 0);
    const next = press(drag, event.pointerId, primary);
    if (next === drag) {
      return false;
    }
    drag = next;
    aimAt(event, rect);
    return true;
  },

  move(event, rect) {
    if (steers(drag, event.pointerId)) {
      aimAt(event, rect);
    }
  },

  release(event) {
    if (!steers(drag, event.pointerId)) {
      return false;
    }
    drag = release(drag, event.pointerId);
    return true;
  },

  send(match) {
    match.set_pointer(pointer[0], pointer[1]);
  },

  sideColors() {
    return [colors.you, colors.bot];
  },
};
