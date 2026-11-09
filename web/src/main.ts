import init, { WebMatch } from "./wasm/feint_web.js";
import { due, makeClock } from "./clock.ts";
import { idle, press, release, steers, type Drag } from "./drag.ts";
import { fit, pixels, toCanvas, toTable, TABLE, type Fit, type Table } from "./geometry.ts";
import { countdown, follow, goalBanner, shade } from "./hud.ts";
import { localSeed, profilePath, profileVersion, seedRequest, ticketFrom, uploadPath, type Ticket } from "./api.ts";
import { describe } from "./overlay.ts";
import { again, finish, first, start, type Screen } from "./screens.ts";

const TICK_CAP = 12;
const NICK_KEY = "feint.nick";
const POINTER_TICKS = 4;
const PAD = 30;
const COUNTDOWN_STEP = 600;
const TRAIL = 10;
const HOLE_SPACING = 0.05;

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const canvas = $<HTMLCanvasElement>("table");
const ctx = canvas.getContext("2d") as CanvasRenderingContext2D;
const debugBox = $<HTMLPreElement>("debug");
const dragHint = $<HTMLParagraphElement>("drag-hint");
const params = new URLSearchParams(location.search);
const speed = Math.max(1, Math.min(16, Number(params.get("speed")) || 1));

interface Colors {
  you: string;
  bot: string;
  puck: string;
  surface: string;
  surfaceDeep: string;
  rail: string;
  railLight: string;
  line: string;
  hole: string;
  ink: string;
}

let screen: Screen = first(readNick());
let game: WebMatch | null = null;
let clock = makeClock();
let view = fit({ width: 1, height: 1, dpr: 1 });
let dpr = 1;
let pointer: [number, number] = [-0.8, 0];
let drag: Drag = idle;
let showDebug = false;
let table: Table = TABLE;
let sizes = { mouth: 0.15, puck: 0.05, mallet: 0.07 };
let colors = readColors();
let surface: HTMLCanvasElement | null = null;
let lastRecord: Uint8Array | null = null;
let starting = false;
let pointerTick = -Infinity;
let startAt = 0;
let scorer: 0 | 1 | null = null;
let pauseFull = 1;
let trail: Array<[number, number]> = [];
let playing: { ticket: Ticket | null; profile: number | null } = { ticket: null, profile: null };

function readNick(): string {
  try {
    return localStorage.getItem(NICK_KEY) ?? "";
  } catch {
    return "";
  }
}

function saveNick(nick: string) {
  try {
    localStorage.setItem(NICK_KEY, nick);
  } catch {
    return;
  }
}

function css(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

function readColors(): Colors {
  return {
    you: css("--you"),
    bot: css("--bot"),
    puck: css("--puck"),
    surface: css("--surface"),
    surfaceDeep: css("--surface-deep"),
    rail: css("--rail"),
    railLight: css("--rail-light"),
    line: css("--line"),
    hole: css("--hole"),
    ink: css("--ink"),
  };
}

function show(next: Screen) {
  screen = next;
  for (const name of ["level", "game", "result"]) {
    $(name).hidden = screen.name !== name;
  }
  if (screen.name === "level") {
    $<HTMLInputElement>("nick").value = screen.nick;
  }
  if (screen.name === "result") {
    const [you, bot] = screen.score;
    $("verdict").textContent = you > bot ? "You win" : "Feint wins";
    $("verdict").style.color = you > bot ? colors.you : colors.bot;
    $("final").textContent = `${you} : ${bot} · level ${screen.level} · ${screen.nick}`;
  }
}

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

function resize() {
  const rect = canvas.getBoundingClientRect();
  dpr = window.devicePixelRatio || 1;
  const v = { width: rect.width, height: rect.height, dpr };
  const size = pixels(v);
  canvas.width = size.width;
  canvas.height = size.height;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  view = fit(v, table, Math.min(PAD + 40, 24 + 0.05 * Math.min(rect.width, rect.height)));
  colors = readColors();
  surface = paintSurface(rect.width, rect.height);
}

function aimAt(event: PointerEvent) {
  const rect = canvas.getBoundingClientRect();
  pointer = toTable(view, event.clientX - rect.left, event.clientY - rect.top, table);
}

function disc(x: number, y: number, r: number, inner: string, outer: string, glow: string) {
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

function mallet(x: number, y: number, color: string) {
  const [cx, cy, radius] = disc(x, y, sizes.mallet, color, shade(color, 0.55), color);
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

function puck(x: number, y: number) {
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
  const [cx, cy] = disc(x, y, sizes.puck, "#ffffff", "#c9d3e3", "rgba(255,255,255,0.6)");
  ctx.strokeStyle = "rgba(10,20,40,0.25)";
  ctx.lineWidth = Math.max(1, radius * 0.12);
  ctx.beginPath();
  ctx.arc(cx, cy, radius * 0.6, 0, Math.PI * 2);
  ctx.stroke();
}

function centreText(text: string, color: string, size: number, offset = 0) {
  const [cx, cy] = at(0, 0);
  ctx.save();
  ctx.font = `800 ${size}px system-ui, sans-serif`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.shadowColor = color;
  ctx.shadowBlur = 24;
  ctx.fillStyle = color;
  ctx.fillText(text, cx, cy + offset);
  ctx.restore();
}

function draw(match: WebMatch, now: number) {
  const rect = canvas.getBoundingClientRect();
  ctx.clearRect(0, 0, rect.width, rect.height);
  if (surface) {
    ctx.drawImage(surface, 0, 0, rect.width, rect.height);
  }
  const p = match.positions();
  mallet(p[4], p[5], colors.you);
  mallet(p[8], p[9], colors.bot);
  puck(p[0], p[1]);
  const big = Math.max(24, view.scale * 0.08);
  const banner = goalBanner(match.serve_pause(), pauseFull, scorer);
  if (banner) {
    const color = banner.side === 0 ? colors.you : colors.bot;
    centreText(banner.text, color, big);
    const [cx, cy] = at(0, 0);
    const width = view.scale * 0.5;
    ctx.fillStyle = "rgba(255,255,255,0.12)";
    ctx.fillRect(cx - width / 2, cy + big * 0.7, width, 4);
    ctx.fillStyle = color;
    ctx.fillRect(cx - width / 2, cy + big * 0.7, width * banner.progress, 4);
  }
  const count = countdown(startAt - now, COUNTDOWN_STEP);
  if (count) {
    centreText(count, colors.ink, big * 2.8);
  }
  if (showDebug) {
    drawDebug(match);
  }
}

function drawDebug(match: WebMatch) {
  const insight = {
    intent: match.intent(),
    memory: match.memory(),
    likelyShot: match.likely_shot(),
    likelyCover: Array.from(match.likely_cover()),
    shown: match.current_shown(),
    real: match.current_real(),
  };
  debugBox.textContent = describe(insight).join("\n");
  const mark = (attacker: number, cls: number, color: string, filled: boolean) => {
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
  };
  mark(0, insight.likelyShot, colors.you, false);
  mark(1, insight.shown, colors.bot, false);
  mark(1, insight.real, colors.bot, true);
}

function step(match: WebMatch, now: number) {
  if (now < startAt) {
    clock = makeClock();
    return;
  }
  const ticks = due(clock, now, document.hidden, match.tick_hz(), TICK_CAP * speed, speed);
  for (let done = 0; done < ticks; ) {
    if (match.tick() >= pointerTick + POINTER_TICKS) {
      match.set_pointer(pointer[0], pointer[1]);
      pointerTick = match.tick();
    }
    const n = Math.min(ticks - done, POINTER_TICKS);
    const [youBefore, botBefore] = Array.from(match.score());
    if (match.advance(n) > 0) {
      const [you, bot] = Array.from(match.score());
      scorer = you > youBefore ? 0 : bot > botBefore ? 1 : scorer;
      pauseFull = Math.max(match.serve_pause(), 1e-3);
      trail = [];
    }
    if (match.serve_pause() <= 0) {
      scorer = null;
    }
    done += n;
  }
}

function frame(now: number) {
  if (screen.name === "game" && game) {
    step(game, now);
    const [you, bot] = Array.from(game.score());
    $("score-you").textContent = String(you);
    $("score-bot").textContent = String(bot);
    draw(game, now);
    if (game.finished()) {
      lastRecord = game.record();
      game.free();
      game = null;
      endDrag();
      show(finish(screen, [you, bot]));
      void upload(lastRecord);
    }
  }
  requestAnimationFrame(frame);
}

async function fetchTicket(): Promise<Ticket | null> {
  try {
    const response = await fetch("/api/seed", seedRequest());
    return response.ok ? ticketFrom(await response.json()) : null;
  } catch {
    return null;
  }
}

async function fetchProfile(nick: string): Promise<{ version: number; bytes: Uint8Array } | null> {
  try {
    const response = await fetch(profilePath(nick));
    const version = profileVersion(response.headers.get("x-profile-version"));
    if (response.status !== 200 || version === null) {
      return null;
    }
    return { version, bytes: new Uint8Array(await response.arrayBuffer()) };
  } catch {
    return null;
  }
}

async function upload(record: Uint8Array) {
  const saved = $("saved");
  const { ticket, profile } = playing;
  if (ticket === null || screen.name !== "result") {
    saved.textContent = "not saved: no arena";
    return;
  }
  saved.textContent = "saving…";
  try {
    const response = await fetch(uploadPath(screen.nick, ticket.ticket, profile), {
      method: "POST",
      headers: { "content-type": "application/octet-stream", "x-feint": "1" },
      body: new Uint8Array(record),
    });
    saved.textContent = response.status === 201 ? "saved" : `not saved: ${response.status}`;
  } catch {
    saved.textContent = "not saved: arena unreachable";
  }
}

async function begin(level: number) {
  const nick = $<HTMLInputElement>("nick").value;
  const next = start(screen, level, nick);
  if (next.name !== "game" || starting) {
    return;
  }
  starting = true;
  saveNick(next.nick);
  const ticket = await fetchTicket();
  const profile = ticket !== null && level === 5 ? await fetchProfile(next.nick) : null;
  const seed = ticket?.seed ?? localSeed(Math.random);
  playing = { ticket, profile: profile?.version ?? null };
  try {
    game = profile ? WebMatch.new_with_profile(level, seed, profile.bytes) : new WebMatch(level, seed);
  } catch {
    playing = { ticket, profile: null };
    game = new WebMatch(level, seed);
  }
  pointerTick = -Infinity;
  starting = false;
  $("saved").textContent = "";
  const [halfLength, halfWidth, mouth, puckR, malletR] = Array.from(game.table());
  table = { halfLength, halfWidth };
  sizes = { mouth, puck: puckR, mallet: malletR };
  clock = makeClock();
  pointer = [-0.8, 0];
  scorer = null;
  trail = [];
  startAt = performance.now() + 3 * COUNTDOWN_STEP;
  dragHint.hidden = false;
  $("who").textContent = next.nick;
  $("lvl").textContent = `level ${level}`;
  show(next);
  resize();
}

function endDrag() {
  drag = idle;
  canvas.classList.remove("dragging");
}

function main() {
  canvas.addEventListener("pointerdown", (e) => {
    const primary = e.isPrimary && (e.pointerType !== "mouse" || e.button === 0);
    const next = press(drag, e.pointerId, primary);
    if (next === drag) {
      return;
    }
    e.preventDefault();
    drag = next;
    canvas.setPointerCapture(e.pointerId);
    canvas.classList.add("dragging");
    dragHint.hidden = true;
    aimAt(e);
  });
  canvas.addEventListener("pointermove", (e) => {
    if (steers(drag, e.pointerId)) {
      aimAt(e);
    }
  });
  const end = (e: PointerEvent) => {
    if (steers(drag, e.pointerId)) {
      drag = release(drag, e.pointerId);
      canvas.classList.remove("dragging");
    }
  };
  canvas.addEventListener("pointerup", end);
  canvas.addEventListener("pointercancel", end);
  canvas.addEventListener("lostpointercapture", end);
  canvas.addEventListener("contextmenu", (e) => e.preventDefault());
  window.addEventListener("resize", () => {
    if (screen.name === "game") {
      resize();
    }
  });
  document.addEventListener("visibilitychange", () => {
    clock = makeClock();
  });
  window.addEventListener("keydown", (e) => {
    if (e.key === "d" || e.key === "D") {
      showDebug = !showDebug;
      debugBox.hidden = !showDebug;
    }
  });
}

async function boot() {
  main();
  await init();
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-level]")) {
    button.addEventListener("click", () => void begin(Number(button.dataset.level)));
    button.disabled = false;
  }
  $("again").addEventListener("click", () => show(again(screen)));
  Object.assign(window, { feintRecord: () => lastRecord, feintPositions: () => game?.positions() ?? null });
  show(screen);
  requestAnimationFrame(frame);
}

void boot();
