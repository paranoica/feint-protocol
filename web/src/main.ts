import init, { card_checkpoints, card_games, checkpoints, games, WebCards, WebMatch } from "./wasm/feint_web.js";
import { due, makeClock } from "./clock.ts";
import { pixels } from "./geometry.ts";
import { countdown } from "./hud.ts";
import { localSeed, profilePath, profileVersion, Saves, seedRequest, ticketFrom, uploadPath, type Ticket } from "./api.ts";
import {
  again,
  cards,
  finish,
  leaveCards,
  first,
  parseReplay,
  replay,
  replayLink,
  replayStart,
  start,
  watch,
  type Replay,
  type Screen,
} from "./screens.ts";
import { profileWanted, trainedCheckpoint, trainedOffered, type BotKind } from "./bots.ts";
import { GAMES, gameById } from "./games/index.ts";
import { CARD_TICK_MS, CardBoard, ending, HOW_TO_PLAY } from "./games/believe.ts";
import { fakeLinks, parseComparison, parseFakes, parseResults, renderComparison, renderFakes } from "./compare.ts";
import type { GameView, Pair } from "./games/types.ts";

const TICK_CAP = 12;
const NICK_KEY = "feint.nick";
const COUNTDOWN_STEP = 600;
const WATCH_SPEEDS = [1, 2, 4];
const SKIP_CHUNK = 2400;

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const canvas = $<HTMLCanvasElement>("table");
const ctx = canvas.getContext("2d") as CanvasRenderingContext2D;
const debugBox = $<HTMLPreElement>("debug");
const debugLeft = $<HTMLPreElement>("debug-left");
const dragHint = $<HTMLParagraphElement>("drag-hint");
const params = new URLSearchParams(location.search);
const speed = Math.max(1, Math.min(16, Number(params.get("speed")) || 1));

let screen: Screen = first(readNick());
let game: WebMatch | null = null;
let current: GameView = GAMES[0];
let clock = makeClock();
let showDebug = false;
let lastRecord: Uint8Array | null = null;
let starting = false;
let inputTick = -Infinity;
let startAt = 0;
let scorer: 0 | 1 | null = null;
let pauseFull = 1;
let size = { width: 1, height: 1 };
let playing: { ticket: Ticket | null; profile: number | null } = { ticket: null, profile: null };
const saves = new Saves();
const SAVE_WAIT_MS = 10_000;
let watching: Pair | null = null;
let watchSpeed = 1;
let skipTo = 0;
let table: WebCards | null = null;
let board: CardBoard | null = null;
let cardTimer: ReturnType<typeof setInterval> | null = null;
let cardMatch = 0;

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
  debugBox.classList.toggle("right", watching !== null);
  debugLeft.hidden = !showDebug || watching === null;
  debugBox.hidden = !showDebug;
  for (const name of ["level", "game", "result", "cards"]) {
    $(name).hidden = screen.name !== name;
  }
  if (screen.name === "level") {
    $<HTMLInputElement>("nick").value = screen.nick;
    void loadComparison(screen.nick);
  }
  if (screen.name === "result" && screen.replay) {
    const r = screen.replay;
    const [left, right] = screen.score;
    $("verdict").textContent = "Replay over";
    $("verdict").style.color = "";
    $("final").textContent = `${current.name} · ${left} : ${right} · ${r.kind} level ${r.level} vs the human model · seed ${r.seed}`;
  } else if (screen.name === "result") {
    const [you, bot] = screen.score;
    const pair = screen.watch;
    const [left, right] = current.sideColors();
    $("verdict").textContent = pair
      ? `Level ${you > bot ? pair[0] : pair[1]} (${you > bot ? "left" : "right"}) wins`
      : you > bot
        ? "You win"
        : "Feint wins";
    $("verdict").style.color = you > bot ? left : right;
    $("final").textContent = pair
      ? `${current.name} · ${you} : ${bot} · level ${pair[0]} vs level ${pair[1]}`
      : `${current.name} · ${you} : ${bot} · level ${screen.level}${screen.bot ? " · trained" : ""} · ${screen.nick}`;
  }
}

function resize() {
  const rect = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const px = pixels({ width: rect.width, height: rect.height, dpr });
  canvas.width = px.width;
  canvas.height = px.height;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  size = { width: rect.width, height: rect.height };
  current.resize(rect.width, rect.height, dpr);
}

function step(match: WebMatch, now: number) {
  if (match.tick() < skipTo) {
    match.advance(Math.min(SKIP_CHUNK, skipTo - match.tick()));
    clock = makeClock();
    return;
  }
  if (now < startAt) {
    clock = makeClock();
    return;
  }
  const rate = speed * (watching ? watchSpeed : 1);
  const ticks = due(clock, now, document.hidden, match.tick_hz(), TICK_CAP * rate, rate);
  const every = Math.max(1, current.inputTicks);
  for (let done = 0; done < ticks; ) {
    if (!watching && match.tick() >= inputTick + every) {
      current.send(match);
      inputTick = match.tick();
    }
    const n = Math.min(ticks - done, every);
    const [youBefore, botBefore] = Array.from(match.score());
    if (match.advance(n) > 0) {
      const [you, bot] = Array.from(match.score());
      scorer = you > youBefore ? 0 : bot > botBefore ? 1 : scorer;
      pauseFull = Math.max(match.serve_pause(), 1e-3);
    }
    if (match.serve_pause() <= 0) {
      scorer = null;
    }
    done += n;
  }
}

function drawDebug(match: WebMatch) {
  const [left, right] = current.debugLines(match, watching);
  debugBox.textContent = right.join("\n");
  debugLeft.textContent = left ? left.join("\n") : "";
}

function frame(now: number) {
  if (screen.name === "game" && game) {
    step(game, now);
    const [you, bot] = Array.from(game.score());
    $("score-you").textContent = String(you);
    $("score-bot").textContent = String(bot);
    current.draw({
      ctx,
      match: game,
      width: size.width,
      height: size.height,
      debug: showDebug,
      watching,
      scorer,
      pauseFull,
      countdown: countdown(startAt - now, COUNTDOWN_STEP),
    });
    if (showDebug) {
      drawDebug(game);
    }
    if (game.finished()) {
      lastRecord = game.record();
      const arena = current.arena && game.arena();
      game.free();
      game = null;
      canvas.classList.remove("dragging");
      show(finish(screen, [you, bot]));
      if (watching || !arena) {
        $("saved").textContent = "";
      } else {
        saves.track(upload(lastRecord));
      }
    }
  }
  requestAnimationFrame(frame);
}

let comparing = 0;

async function loadComparison(nick: string) {
  const holder = $("compare");
  const turn = ++comparing;
  try {
    const measured = await fetch("./compare.json");
    const comparison = measured.ok ? parseComparison(await measured.json()) : null;
    if (!comparison) {
      holder.hidden ||= turn === comparing;
      return;
    }
    let results = parseResults([]);
    try {
      const mine = await fetch(`/api/results?nick=${encodeURIComponent(nick)}`);
      results = mine.ok ? parseResults(await mine.json()) : results;
    } catch {
      results = parseResults([]);
    }
    let fakes = null;
    try {
      const found = await fetch("./fakes.json");
      fakes = found.ok ? parseFakes(await found.json()) : null;
    } catch {
      fakes = null;
    }
    if (turn !== comparing) {
      return;
    }
    renderComparison(holder, comparison, results, (id) => gameById(id)?.name ?? id);
    if (fakes) {
      const f = fakes;
      renderFakes(holder, f, fakeLinks(f, Array.from(checkpoints())), (l) =>
        replayLink({ kind: "trained", level: f.level, seed: l.seed, seat: l.seat, from: l.from }),
      );
    }
    holder.hidden = false;
  } catch {
    holder.hidden ||= turn === comparing;
  }
}

async function fetchTicket(): Promise<Ticket | null> {
  try {
    const response = await fetch("/api/seed", seedRequest());
    return response.ok ? ticketFrom(await response.json()) : null;
  } catch {
    return null;
  }
}

async function fetchProfile(nick: string, game: string): Promise<{ version: number; bytes: Uint8Array } | null> {
  try {
    const response = await fetch(profilePath(nick, game));
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

async function uploadCards(record: Uint8Array, ticket: Ticket | null, nick: string, id: number) {
  const status = (text: string) => {
    if (id === cardMatch) {
      $("cards-saved").textContent = text;
    }
  };
  if (ticket === null) {
    status("Не сохранено: арена недоступна");
    return;
  }
  status("Сохраняю…");
  try {
    const response = await fetch(uploadPath(nick, ticket.ticket, null), {
      method: "POST",
      headers: { "content-type": "application/octet-stream", "x-feint": "1" },
      body: new Uint8Array(record),
    });
    status(response.status === 201 ? "Сохранено в арене" : `Не сохранено: ${response.status}`);
  } catch {
    status("Не сохранено: арена недоступна");
  }
}

function stopCards() {
  if (cardTimer !== null) {
    clearInterval(cardTimer);
    cardTimer = null;
  }
  table?.free();
  table = null;
  board = null;
}

function cardTick(ticket: Ticket | null, nick: string, id: number) {
  if (!table || !board) {
    return;
  }
  table.advance(1);
  board.render();
  if (!table.finished()) {
    return;
  }
  if (cardTimer !== null) {
    clearInterval(cardTimer);
    cardTimer = null;
  }
  board.render(true);
  lastRecord = table.record();
  const end = ending(table.outcome());
  $("cards-verdict").textContent = end.title;
  $("cards-end").hidden = false;
  if (end.upload) {
    saves.track(uploadCards(lastRecord, ticket, nick, id));
  } else {
    $("cards-saved").textContent = "Не сохранено: партия не закончена";
  }
}

async function beginCards() {
  const next = cards(screen, $<HTMLInputElement>("nick").value);
  const checkpoint = Array.from(card_checkpoints())[0];
  if (next.name !== "cards" || starting || checkpoint === undefined) {
    return;
  }
  starting = true;
  let ticket: Ticket | null = null;
  try {
    saveNick(next.nick);
    await saves.settled(SAVE_WAIT_MS);
    ticket = await fetchTicket();
    stopCards();
    table = new WebCards(ticket?.seed ?? localSeed(Math.random), checkpoint);
  } catch {
    stopCards();
    return;
  } finally {
    starting = false;
  }
  board = new CardBoard($("cards-board"), table);
  $("cards-who").textContent = next.nick;
  $("cards-end").hidden = true;
  $("cards-saved").textContent = "";
  $("cards-verdict").textContent = "";
  show(next);
  board.render(true);
  const nick = next.nick;
  const id = ++cardMatch;
  cardTimer = setInterval(() => cardTick(ticket, nick, id), CARD_TICK_MS);
}

async function begin(view: GameView, level: number, bot: BotKind) {
  const nick = $<HTMLInputElement>("nick").value;
  const next = start(screen, view.id, level, nick, bot);
  const checkpoint = trainedCheckpoint(view.id, Array.from(checkpoints()), level);
  if (next.name !== "game" || starting || (bot === "trained" && checkpoint === null)) {
    return;
  }
  starting = true;
  try {
    saveNick(next.nick);
    await saves.settled(SAVE_WAIT_MS);
    const ticket = view.arena ? await fetchTicket() : null;
    const profile = profileWanted(bot, level, ticket !== null) ? await fetchProfile(next.nick, view.id) : null;
    const seed = ticket?.seed ?? localSeed(Math.random);
    playing = { ticket, profile: profile?.version ?? null };
    try {
      game =
        bot === "trained" && checkpoint !== null
          ? WebMatch.create_trained(view.id, level, seed, checkpoint)
          : profile
            ? WebMatch.create_with_profile(view.id, level, seed, profile.bytes)
            : WebMatch.create(view.id, level, seed);
    } catch {
      if (bot === "trained") {
        throw new Error("the trained bot did not start");
      }
      playing = { ticket, profile: null };
      game = WebMatch.create(view.id, level, seed);
    }
  } catch {
    game = null;
    return;
  } finally {
    starting = false;
  }
  if (!game) {
    return;
  }
  watching = null;
  enter(view, game, next);
  dragHint.textContent = view.hint;
  dragHint.hidden = view.hint === "";
  $("who").textContent = next.nick;
  $("lvl").textContent = bot === "trained" ? `level ${level} · trained` : `level ${level}`;
}

function enter(view: GameView, match: WebMatch, next: Screen) {
  current = view;
  skipTo = 0;
  inputTick = -Infinity;
  scorer = null;
  clock = makeClock();
  startAt = performance.now() + 3 * COUNTDOWN_STEP;
  $("saved").textContent = "";
  view.setup(match);
  show(next);
  resize();
}

function speedLabel(): string {
  if (screen.name === "game" && screen.replay) {
    return `replay · seed ${screen.replay.seed} · ×${watchSpeed} (F)`;
  }
  return watching ? `level ${watching[1]} · ×${watchSpeed} (F)` : "";
}

function beginReplay(view: GameView, r: Replay) {
  const next = replay(screen, view.id, r, $<HTMLInputElement>("nick").value);
  if (next.name !== "game" || starting) {
    return;
  }
  try {
    game = WebMatch.rerun(r.kind, r.level, r.seed, r.seat);
  } catch {
    game = null;
    return;
  }
  playing = { ticket: null, profile: null };
  watching = r.seat === 0 ? [r.level, 5] : [5, r.level];
  watchSpeed = 1;
  enter(view, game, next);
  startAt = performance.now();
  skipTo = replayStart(r);
  dragHint.hidden = true;
  $("who").textContent = r.seat === 0 ? `${r.kind} level ${r.level}` : "human model";
  $("lvl").textContent = speedLabel();
}

function beginWatch(view: GameView, left: number, right: number) {
  const next = watch(screen, view.id, left, right, $<HTMLInputElement>("nick").value);
  if (next.name !== "game" || starting) {
    return;
  }
  try {
    game = WebMatch.watch(view.id, left, right, localSeed(Math.random));
  } catch {
    game = null;
    return;
  }
  playing = { ticket: null, profile: null };
  watching = [left, right];
  watchSpeed = 1;
  enter(view, game, next);
  dragHint.hidden = true;
  $("who").textContent = `level ${left}`;
  $("lvl").textContent = speedLabel();
}

function levelOptions(selected: number): string[] {
  return [1, 2, 3, 4, 5].map((l) => (l === selected ? `${l}*` : `${l}`));
}

function select(label: string, selected: number): HTMLSelectElement {
  const box = document.createElement("select");
  box.setAttribute("aria-label", label);
  for (const option of levelOptions(selected)) {
    const o = document.createElement("option");
    const value = option.replace("*", "");
    o.value = value;
    o.textContent = value;
    o.selected = option.endsWith("*");
    box.append(o);
  }
  return box;
}

function buildHub() {
  const holder = $("games");
  holder.replaceChildren();
  const names = ["Rookie", "Novice", "Amateur", "Pro", "Feint"];
  const built = new Set(Array.from(games()));
  for (const view of GAMES.filter((g) => built.has(g.id))) {
    const card = document.createElement("article");
    card.className = "card game-card";
    const title = document.createElement("h2");
    title.textContent = view.name;
    const pitch = document.createElement("p");
    pitch.className = "tagline";
    pitch.textContent = view.pitch;
    const levels = document.createElement("div");
    levels.className = "levels";
    let bot: BotKind = "hand-made";
    const kinds = document.createElement("div");
    kinds.className = "bots";
    kinds.hidden = !trainedOffered(view.id, Array.from(checkpoints()));
    for (const [kind, label] of [
      ["hand-made", "Hand-made bot"],
      ["trained", "Trained bot"],
    ] as const) {
      const button = document.createElement("button");
      button.dataset.bot = kind;
      button.textContent = label;
      button.setAttribute("aria-pressed", String(kind === bot));
      button.addEventListener("click", () => {
        bot = kind;
        for (const b of kinds.querySelectorAll("button")) {
          b.setAttribute("aria-pressed", String(b.dataset.bot === bot));
        }
      });
      kinds.append(button);
    }
    names.forEach((name, i) => {
      const button = document.createElement("button");
      button.dataset.game = view.id;
      button.dataset.level = String(i + 1);
      const b = document.createElement("b");
      b.textContent = String(i + 1);
      const span = document.createElement("span");
      span.textContent = name;
      button.append(b, span);
      button.addEventListener("click", () => void begin(view, i + 1, bot));
      levels.append(button);
    });
    const watchRow = document.createElement("div");
    watchRow.className = "watch";
    const caption = document.createElement("span");
    caption.textContent = "Watch two bots";
    const left = select("left bot level", 1);
    const versus = document.createElement("span");
    versus.textContent = "vs";
    const right = select("right bot level", 5);
    const go = document.createElement("button");
    go.textContent = "Watch";
    go.dataset.watch = view.id;
    go.addEventListener("click", () => beginWatch(view, Number(left.value), Number(right.value)));
    watchRow.append(caption, left, versus, right, go);
    card.append(title, pitch, kinds, levels, watchRow);
    holder.append(card);
  }
  buildCardsCard(holder);
}

function buildCardsCard(holder: HTMLElement) {
  if (!Array.from(card_games()).includes("believe") || card_checkpoints().length === 0) {
    return;
  }
  const card = document.createElement("article");
  card.className = "card game-card";
  const title = document.createElement("h2");
  title.textContent = "Верю — не верю";
  const pitch = document.createElement("p");
  pitch.className = "tagline";
  pitch.textContent = "Карточный блеф против обученного бота: кладите карты рубашкой вверх и врите убедительно.";
  const rules = document.createElement("details");
  rules.className = "cards-rules";
  const summary = document.createElement("summary");
  summary.textContent = "Как играть";
  const list = document.createElement("ol");
  for (const line of HOW_TO_PLAY) {
    const item = document.createElement("li");
    item.textContent = line;
    list.append(item);
  }
  rules.append(summary, list);
  const play = document.createElement("button");
  play.className = "primary";
  play.dataset.cards = "believe";
  play.textContent = "Играть";
  play.addEventListener("click", () => void beginCards());
  card.append(title, pitch, rules, play);
  holder.append(card);
}

function main() {
  canvas.addEventListener("pointerdown", (e) => {
    if (watching || !game) {
      return;
    }
    if (!current.press(e, canvas.getBoundingClientRect())) {
      return;
    }
    e.preventDefault();
    canvas.setPointerCapture(e.pointerId);
    canvas.classList.add("dragging");
    dragHint.hidden = true;
  });
  canvas.addEventListener("pointermove", (e) => current.move(e, canvas.getBoundingClientRect()));
  const end = (e: PointerEvent) => {
    if (current.release(e)) {
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
    if (game && !watching && current.key?.(e)) {
      e.preventDefault();
      return;
    }
    if (e.key === "d" || e.key === "D") {
      showDebug = !showDebug;
      debugBox.hidden = !showDebug;
      debugLeft.hidden = !showDebug || !watching;
    }
    if ((e.key === "f" || e.key === "F") && watching) {
      watchSpeed = WATCH_SPEEDS[(WATCH_SPEEDS.indexOf(watchSpeed) + 1) % WATCH_SPEEDS.length];
      $("lvl").textContent = speedLabel();
    }
  });
}

async function boot() {
  main();
  await init();
  buildHub();
  $("again").addEventListener("click", () => show(again(screen)));
  $("cards-leave").addEventListener("click", () => {
    stopCards();
    show(leaveCards(screen));
  });
  Object.assign(window, {
    feintRecord: () => lastRecord,
    feintPositions: () => game?.view() ?? null,
    feintGames: () => GAMES.map((g) => g.id),
    feintGame: (id: string) => gameById(id)?.name ?? null,
  });
  show(screen);
  const asked = parseReplay(location.search);
  const hockey = gameById("airhockey");
  if (asked && hockey) {
    beginReplay(hockey, asked);
  }
  requestAnimationFrame(frame);
}

void boot();
