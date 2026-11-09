export interface Drag {
  pointer: number | null;
}

export const idle: Drag = { pointer: null };

export function press(drag: Drag, pointer: number, primary: boolean): Drag {
  return drag.pointer === null && primary ? { pointer } : drag;
}

export function steers(drag: Drag, pointer: number): boolean {
  return drag.pointer === pointer;
}

export function release(drag: Drag, pointer: number): Drag {
  return drag.pointer === pointer ? idle : drag;
}
