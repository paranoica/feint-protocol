import { hockey } from "./hockey.ts";
import type { GameView } from "./types.ts";

export const GAMES: GameView[] = [hockey];

export function gameById(id: string): GameView | undefined {
  return GAMES.find((g) => g.id === id);
}
