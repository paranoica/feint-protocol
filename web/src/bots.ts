export type BotKind = "hand-made" | "trained";

export const BOT_KINDS: readonly BotKind[] = ["hand-made", "trained"];

export function trainedCheckpoint(game: string, checkpoints: readonly string[]): string | null {
  return game === "airhockey" && checkpoints.length > 0 ? checkpoints[0] : null;
}

export function profileWanted(bot: BotKind, level: number, ticketed: boolean): boolean {
  return bot === "hand-made" && level === 5 && ticketed;
}
