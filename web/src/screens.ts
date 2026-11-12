import { BOT_KINDS, type BotKind } from "./bots.ts";

export type Watch = [number, number];

export interface Replay {
  kind: BotKind;
  level: number;
  seed: number;
  seat: 0 | 1;
  from: number;
}

export type Screen =
  | { name: "level"; nick: string }
  | {
      name: "game";
      game: string;
      nick: string;
      level: number;
      watch?: Watch;
      bot?: "trained";
      replay?: Replay;
    }
  | {
      name: "result";
      game: string;
      nick: string;
      level: number;
      score: [number, number];
      watch?: Watch;
      bot?: "trained";
      replay?: Replay;
    };

export const REPLAY_LEAD = 480;
const MAX_SEED = 0xffffffff;
const MAX_TICK = 240 * 600;

function whole(raw: string | null, max: number): number | null {
  if (raw === null || !/^[0-9]{1,10}$/.test(raw)) {
    return null;
  }
  const n = Number(raw);
  return n <= max ? n : null;
}

export function parseReplay(search: string): Replay | null {
  const p = new URLSearchParams(search);
  const kind = p.get("replay");
  const level = whole(p.get("level"), 5);
  const seed = whole(p.get("seed"), MAX_SEED);
  const seat = p.get("seat");
  const from = whole(p.get("from"), MAX_TICK);
  if (kind !== "trained" && kind !== "hand-made") {
    return null;
  }
  if (level === null || level < 1 || seed === null || from === null || (seat !== "0" && seat !== "1")) {
    return null;
  }
  return { kind, level, seed, seat: seat === "0" ? 0 : 1, from };
}

export function replayLink(r: Replay): string {
  return `/?replay=${r.kind}&level=${r.level}&seed=${r.seed}&seat=${r.seat}&from=${r.from}`;
}

export function replayStart(r: Replay): number {
  return Math.max(0, r.from - REPLAY_LEAD);
}

const isLevel = (level: number) => Number.isInteger(level) && level >= 1 && level <= 5;

export const MAX_NICK = 24;

export function cleanNick(raw: string): string {
  const visible = raw.replace(/[\u0000-\u001f\u007f-\u009f\u200e\u200f\u202a-\u202e\u2066-\u2069]/g, "");
  const nick = Array.from(visible.trim()).slice(0, MAX_NICK).join("").trim();
  return nick === "" ? "player" : nick;
}

export function first(nick: string): Screen {
  return { name: "level", nick: cleanNick(nick) };
}

export function start(
  screen: Screen,
  game: string,
  level: number,
  nick: string,
  bot: BotKind = "hand-made",
): Screen {
  if (screen.name !== "level" || game === "" || !isLevel(level) || !BOT_KINDS.includes(bot)) {
    return screen;
  }
  const next: Screen = { name: "game", game, nick: cleanNick(nick), level };
  return bot === "trained" ? { ...next, bot } : next;
}

export function watch(screen: Screen, game: string, left: number, right: number, nick: string): Screen {
  if (screen.name !== "level" || game === "" || !isLevel(left) || !isLevel(right)) {
    return screen;
  }
  return { name: "game", game, nick: cleanNick(nick), level: right, watch: [left, right] };
}

export function replay(screen: Screen, game: string, r: Replay, nick: string): Screen {
  if (screen.name !== "level" || game === "") {
    return screen;
  }
  return { name: "game", game, nick: cleanNick(nick), level: r.level, replay: r };
}

export function finish(screen: Screen, score: [number, number]): Screen {
  if (screen.name !== "game") {
    return screen;
  }
  const result: Screen = { name: "result", game: screen.game, nick: screen.nick, level: screen.level, score };
  const shown = screen.bot ? { ...result, bot: screen.bot } : result;
  const watched = screen.watch ? { ...shown, watch: screen.watch } : shown;
  return screen.replay ? { ...watched, replay: screen.replay } : watched;
}

export function again(screen: Screen): Screen {
  if (screen.name !== "result") {
    return screen;
  }
  return { name: "level", nick: screen.nick };
}
