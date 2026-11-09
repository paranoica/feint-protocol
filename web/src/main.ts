import init, { WebMatch } from "./wasm/feint_web.js";
import { due, makeClock } from "./clock.ts";
import { fit, pixels, toCanvas, toTable, TABLE, type Fit } from "./geometry.ts";
import { describe } from "./overlay.ts";
import { again, finish, first, start, type Screen } from "./screens.ts";

const TICK_CAP = 12;
const NICK_KEY = "feint.nick";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const canvas = $<HTMLCanvasElement>("table");
const ctx = canvas.getContext("2d") as CanvasRenderingContext2D;
const debugBox = $<HTMLPreElement>("debug");
const params = new URLSearchParams(location.search);
const speed = Math.max(1, Math.min(16, Number(params.get("speed")) || 1));

let screen: Screen = first(readNick());
let game: WebMatch | null = null;
let clock = makeClock();
let view = fit({ width: 1, height: 1, dpr: 1 });
let pointer: [number, number] = [-0.8, 0];
let showDebug = false;
let lastRecord: Uint8Array | null = null;

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
    $("final").textContent = `${you} : ${bot} · level ${screen.level} · ${screen.nick}`;
  }
}

function resize() {
  const rect = canvas.getBoundingClientRect();
  const v = { width: rect.width, height: rect.height, dpr: window.devicePixelRatio || 1 };
  const size = pixels(v);
  canvas.width = size.width;
  canvas.height = size.height;
  ctx.setTransform(v.dpr, 0, 0, v.dpr, 0, 0);
  view = fit(v);
}

function aim(event: PointerEvent) {
  const rect = canvas.getBoundingClientRect();
  pointer = toTable(view, event.clientX - rect.left, event.clientY - rect.top);
}

function circle(f: Fit, x: number, y: number, r: number, color: string) {
  const [cx, cy] = toCanvas(f, x, y);
  ctx.beginPath();
  ctx.arc(cx, cy, r * f.scale, 0, Math.PI * 2);
  ctx.fillStyle = color;
  ctx.fill();
}

function css(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

function draw(match: WebMatch) {
  const f = view;
  const rect = canvas.getBoundingClientRect();
  ctx.clearRect(0, 0, rect.width, rect.height);
  const [x0, y0] = toCanvas(f, TABLE.halfLength, TABLE.halfWidth);
  const [x1, y1] = toCanvas(f, -TABLE.halfLength, -TABLE.halfWidth);
  ctx.fillStyle = css("--table");
  ctx.fillRect(x0, y0, x1 - x0, y1 - y0);
  ctx.strokeStyle = css("--line");
  ctx.lineWidth = 2;
  ctx.strokeRect(x0, y0, x1 - x0, y1 - y0);
  const [, midY] = toCanvas(f, 0, 0);
  ctx.beginPath();
  ctx.moveTo(x0, midY);
  ctx.lineTo(x1, midY);
  ctx.stroke();
  const mouth = 0.125 * f.scale;
  const [gx, top] = toCanvas(f, TABLE.halfLength, 0);
  const [, bottom] = toCanvas(f, -TABLE.halfLength, 0);
  ctx.lineWidth = 6;
  ctx.strokeStyle = css("--bot");
  ctx.beginPath();
  ctx.moveTo(gx - mouth, top);
  ctx.lineTo(gx + mouth, top);
  ctx.stroke();
  ctx.strokeStyle = css("--you");
  ctx.beginPath();
  ctx.moveTo(gx - mouth, bottom);
  ctx.lineTo(gx + mouth, bottom);
  ctx.stroke();
  const p = match.positions();
  circle(f, p[4], p[5], 0.05, css("--you"));
  circle(f, p[8], p[9], 0.05, css("--bot"));
  circle(f, p[0], p[1], 0.04, css("--puck"));
  if (showDebug) {
    drawDebug(match, f);
  }
}

function drawDebug(match: WebMatch, f: Fit) {
  const insight = {
    intent: match.intent(),
    memory: match.memory(),
    likelyShot: match.likely_shot(),
    likelyCover: Array.from(match.likely_cover()),
  };
  debugBox.textContent = describe(insight).join("\n");
  if (insight.likelyShot >= 0) {
    const [x, y] = Array.from(match.class_point(0, insight.likelyShot));
    const [cx, cy] = toCanvas(f, x, y);
    ctx.strokeStyle = css("--you");
    ctx.lineWidth = 2;
    ctx.strokeRect(cx - 8, cy - 8, 16, 16);
  }
}

function frame(now: number) {
  if (screen.name === "game" && game) {
    const ticks = due(clock, now, document.hidden, game.tick_hz(), TICK_CAP * speed, speed);
    game.set_pointer(pointer[0], pointer[1]);
    game.advance(ticks);
    const [you, bot] = Array.from(game.score());
    $("score").textContent = `${you} : ${bot}`;
    draw(game);
    if (game.finished()) {
      lastRecord = game.record();
      game.free();
      game = null;
      show(finish(screen, [you, bot]));
    }
  }
  requestAnimationFrame(frame);
}

function begin(level: number) {
  const nick = $<HTMLInputElement>("nick").value;
  const next = start(screen, level, nick);
  if (next.name !== "game") {
    return;
  }
  saveNick(next.nick);
  game = new WebMatch(level, (Math.random() * 2 ** 32) >>> 0);
  clock = makeClock();
  pointer = [-0.8, 0];
  $("who").textContent = `${next.nick} vs level ${level}`;
  show(next);
  resize();
}

async function main() {
  await init();
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-level]")) {
    button.addEventListener("click", () => begin(Number(button.dataset.level)));
  }
  $("again").addEventListener("click", () => show(again(screen)));
  canvas.addEventListener("pointerdown", (e) => canvas.setPointerCapture(e.pointerId));
  window.addEventListener("pointermove", aim);
  window.addEventListener("resize", resize);
  window.addEventListener("keydown", (e) => {
    if (e.key === "d" || e.key === "D") {
      showDebug = !showDebug;
      debugBox.hidden = !showDebug;
    }
  });
  Object.assign(window, { feintRecord: () => lastRecord, feintPositions: () => game?.positions() ?? null });
  show(screen);
  requestAnimationFrame(frame);
}

void main();
