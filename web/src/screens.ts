import { BOT_KINDS, type BotKind } from "./bots.ts";

export type Watch = [number, number];

export type Screen =
  | { name: "level"; nick: string }
  | { name: "game"; game: string; nick: string; level: number; watch?: Watch; bot?: "trained" }
  | {
      name: "result";
      game: string;
      nick: string;
      level: number;
      score: [number, number];
      watch?: Watch;
      bot?: "trained";
    };

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

export function finish(screen: Screen, score: [number, number]): Screen {
  if (screen.name !== "game") {
    return screen;
  }
  const result: Screen = { name: "result", game: screen.game, nick: screen.nick, level: screen.level, score };
  const shown = screen.bot ? { ...result, bot: screen.bot } : result;
  return screen.watch ? { ...shown, watch: screen.watch } : shown;
}

export function again(screen: Screen): Screen {
  if (screen.name !== "result") {
    return screen;
  }
  return { name: "level", nick: screen.nick };
}
