import { hockey } from "./hockey.ts";
import { duelView } from "./duel.ts";
import { penaltyView } from "./penalty.ts";
import { pingpongView } from "./pingpong.ts";
import { boxingView } from "./boxing.ts";
import type { GameView } from "./types.ts";

export const GAMES: GameView[] = [hockey, penaltyView, duelView, pingpongView, boxingView];

export function gameById(id: string): GameView | undefined {
  return GAMES.find((g) => g.id === id);
}
