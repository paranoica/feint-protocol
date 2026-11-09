export type Screen =
  | { name: "level"; nick: string }
  | { name: "game"; nick: string; level: number }
  | { name: "result"; nick: string; level: number; score: [number, number] };

export const MAX_NICK = 24;

export function cleanNick(raw: string): string {
  const visible = raw.replace(/[\u0000-\u001f\u007f-\u009f\u200e\u200f\u202a-\u202e\u2066-\u2069]/g, "");
  const nick = Array.from(visible.trim()).slice(0, MAX_NICK).join("").trim();
  return nick === "" ? "player" : nick;
}

export function first(nick: string): Screen {
  return { name: "level", nick: cleanNick(nick) };
}

export function start(screen: Screen, level: number, nick: string): Screen {
  if (screen.name !== "level" || !Number.isInteger(level) || level < 1 || level > 5) {
    return screen;
  }
  return { name: "game", nick: cleanNick(nick), level };
}

export function finish(screen: Screen, score: [number, number]): Screen {
  if (screen.name !== "game") {
    return screen;
  }
  return { name: "result", nick: screen.nick, level: screen.level, score };
}

export function again(screen: Screen): Screen {
  if (screen.name !== "result") {
    return screen;
  }
  return { name: "level", nick: screen.nick };
}
