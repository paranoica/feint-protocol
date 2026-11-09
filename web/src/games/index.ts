import { hockey } from "./hockey.ts";
import { penaltyView } from "./penalty.ts";
import type { GameView } from "./types.ts";

export const GAMES: GameView[] = [hockey, penaltyView];

export function gameById(id: string): GameView | undefined {
  return GAMES.find((g) => g.id === id);
}
