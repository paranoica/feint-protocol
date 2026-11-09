import type { WebMatch } from "../wasm/feint_web.js";

export type Pair = [number, number];

export interface Scene {
  ctx: CanvasRenderingContext2D;
  match: WebMatch;
  width: number;
  height: number;
  debug: boolean;
  watching: Pair | null;
  scorer: 0 | 1 | null;
  pauseFull: number;
  countdown: string | null;
}

export interface GameView {
  id: string;
  name: string;
  pitch: string;
  hint: string;
  arena: boolean;
  inputTicks: number;
  setup(match: WebMatch): void;
  resize(width: number, height: number, dpr: number): void;
  draw(scene: Scene): void;
  debugLines(match: WebMatch, watching: Pair | null): [string[] | null, string[]];
  press(event: PointerEvent, rect: DOMRect): boolean;
  move(event: PointerEvent, rect: DOMRect): void;
  release(event: PointerEvent): boolean;
  send(match: WebMatch): void;
  sideColors(): [string, string];
}
