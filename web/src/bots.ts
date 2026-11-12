export type BotKind = "hand-made" | "trained";

export const BOT_KINDS: readonly BotKind[] = ["hand-made", "trained"];

export const LEVELS = 5;

export function trainedOffered(game: string, checkpoints: readonly string[]): boolean {
  return game === "airhockey" && checkpoints.length === LEVELS;
}

export function trainedCheckpoint(game: string, checkpoints: readonly string[], level: number): string | null {
  if (!trainedOffered(game, checkpoints) || !Number.isInteger(level) || level < 1 || level > LEVELS) {
    return null;
  }
  return checkpoints[level - 1];
}

export function profileWanted(bot: BotKind, level: number, ticketed: boolean): boolean {
  return bot === "hand-made" && level === 5 && ticketed;
}
