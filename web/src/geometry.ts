export interface Table {
  halfLength: number;
  halfWidth: number;
}

export interface View {
  width: number;
  height: number;
  dpr: number;
}

export interface Fit {
  scale: number;
  left: number;
  top: number;
}

export const TABLE: Table = { halfLength: 1, halfWidth: 0.5 };

export function fit(view: View, table: Table = TABLE): Fit {
  const scale = Math.min(view.width / (2 * table.halfWidth), view.height / (2 * table.halfLength));
  return {
    scale,
    left: (view.width - 2 * table.halfWidth * scale) / 2,
    top: (view.height - 2 * table.halfLength * scale) / 2,
  };
}

export function pixels(view: View): { width: number; height: number } {
  return { width: Math.round(view.width * view.dpr), height: Math.round(view.height * view.dpr) };
}

export function toCanvas(f: Fit, x: number, y: number, table: Table = TABLE): [number, number] {
  return [f.left + (table.halfWidth - y) * f.scale, f.top + (table.halfLength - x) * f.scale];
}

export function toTable(f: Fit, cx: number, cy: number, table: Table = TABLE): [number, number] {
  const x = table.halfLength - (cy - f.top) / f.scale;
  const y = table.halfWidth - (cx - f.left) / f.scale;
  const clamp = (v: number, limit: number) => Math.max(-limit, Math.min(limit, v));
  return [clamp(x, table.halfLength), clamp(y, table.halfWidth)];
}
