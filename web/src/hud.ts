export interface Banner {
  text: string;
  side: 0 | 1;
  progress: number;
}

export function goalBanner(pause: number, full: number, scorer: 0 | 1 | null): Banner | null {
  if (pause <= 0 || scorer === null || full <= 0) {
    return null;
  }
  const progress = Math.max(0, Math.min(1, 1 - pause / full));
  return { text: scorer === 0 ? "Goal!" : "Feint scores", side: scorer, progress };
}

export function countdown(left: number, step: number): string | null {
  return left > 0 ? String(Math.ceil(left / step)) : null;
}

export function shade(hex: string, keep: number): string {
  const m = /^#([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) {
    return hex;
  }
  const n = Number.parseInt(m[1], 16);
  const part = (shift: number) => Math.round(((n >> shift) & 0xff) * Math.max(0, Math.min(1, keep)));
  return `rgb(${part(16)}, ${part(8)}, ${part(0)})`;
}
