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
  horizontal: boolean;
}

export const TABLE: Table = { halfLength: 1, halfWidth: 0.5 };

export function fit(view: View, table: Table = TABLE, pad = 0): Fit {
  const horizontal = view.width > view.height;
  const across = 2 * (horizontal ? table.halfLength : table.halfWidth);
  const down = 2 * (horizontal ? table.halfWidth : table.halfLength);
  const scale = Math.max(0, Math.min((view.width - 2 * pad) / across, (view.height - 2 * pad) / down));
  return {
    scale,
    left: (view.width - across * scale) / 2,
    top: (view.height - down * scale) / 2,
    horizontal,
  };
}

export function pixels(view: View): { width: number; height: number } {
  return { width: Math.round(view.width * view.dpr), height: Math.round(view.height * view.dpr) };
}

export function toCanvas(f: Fit, x: number, y: number, table: Table = TABLE): [number, number] {
  if (f.horizontal) {
    return [f.left + (x + table.halfLength) * f.scale, f.top + (table.halfWidth - y) * f.scale];
  }
  return [f.left + (table.halfWidth - y) * f.scale, f.top + (table.halfLength - x) * f.scale];
}

export function toTable(f: Fit, cx: number, cy: number, table: Table = TABLE): [number, number] {
  const clamp = (v: number, limit: number) => Math.max(-limit, Math.min(limit, v));
  if (f.horizontal) {
    const x = (cx - f.left) / f.scale - table.halfLength;
    const y = table.halfWidth - (cy - f.top) / f.scale;
    return [clamp(x, table.halfLength), clamp(y, table.halfWidth)];
  }
  const x = table.halfLength - (cy - f.top) / f.scale;
  const y = table.halfWidth - (cx - f.left) / f.scale;
  return [clamp(x, table.halfLength), clamp(y, table.halfWidth)];
}
