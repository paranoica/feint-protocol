import type { WebMatch } from "../wasm/feint_web.js";
import { describe, describeBot, intentColor, LEGEND, type Insight } from "../overlay.ts";
import type { GameView, Pair, Scene } from "./types.ts";

interface Layout {
  halfGoal: number;
  reach: number;
  runUp: number;
  wide: number;
}

let layout: Layout = { halfGoal: 3.66, reach: 0.9, runUp: 3, wide: 0.8 };
let width = 1;
let height = 1;
let scale = 40;
let cursor = 0;
let fire = false;
let kickRange = 0.65;
let colors = { you: "#22d3ee", bot: "#fb7185", ink: "#e7eef8", line: "rgba(130,175,255,0.35)", grass: "#0d2a1f" };

function css(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

const centreX = () => width / 2;
const goalY = () => height * 0.3;
const spotY = () => height * 0.62;
const toX = (meters: number) => centreX() + meters * scale;

function insightOf(match: WebMatch, seat: number): Insight {
  return {
    intent: match.intent_of(seat),
    memory: match.memory_of(seat),
    likelyShot: match.likely_shot_of(seat),
    likelyCover: Array.from(match.likely_cover_of(seat)),
    shown: match.shown_of(seat),
    real: match.real_of(seat),
  };
}

function figure(ctx: CanvasRenderingContext2D, x: number, y: number, tilt: number, size: number, color: string) {
  ctx.save();
  ctx.translate(x, y);
  ctx.rotate(tilt);
  ctx.shadowColor = color;
  ctx.shadowBlur = size * 0.6;
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.roundRect(-size * 0.28, -size * 1.1, size * 0.56, size * 1.1, size * 0.2);
  ctx.fill();
  ctx.beginPath();
  ctx.arc(0, -size * 1.35, size * 0.24, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
}

function keeperShape(ctx: CanvasRenderingContext2D, x: number, y: number, dive: number, diving: boolean, color: string) {
  const span = layout.reach * scale;
  ctx.save();
  ctx.translate(x, y);
  ctx.rotate(diving ? dive * 0.9 : 0);
  ctx.shadowColor = color;
  ctx.shadowBlur = 16;
  ctx.fillStyle = color;
  ctx.globalAlpha = 0.25;
  ctx.beginPath();
  ctx.roundRect(-span, -6, span * 2, 12, 6);
  ctx.fill();
  ctx.globalAlpha = 1;
  ctx.beginPath();
  ctx.roundRect(-scale * 0.3, -scale * 0.9, scale * 0.6, scale * 0.9, scale * 0.2);
  ctx.fill();
  ctx.beginPath();
  ctx.arc(0, -scale * 1.1, scale * 0.22, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
}

function banner(ctx: CanvasRenderingContext2D, text: string, color: string) {
  ctx.save();
  ctx.font = `800 ${Math.max(28, scale * 1.4)}px system-ui, sans-serif`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.shadowColor = color;
  ctx.shadowBlur = 24;
  ctx.fillStyle = color;
  ctx.fillText(text, centreX(), height * 0.42);
  ctx.restore();
}

function classNames(): string[] {
  return ["left", "middle", "right"];
}

export const penaltyView: GameView = {
  id: "penalty",
  name: "Penalty shootout",
  pitch: "Five kicks each, then sudden death. The keeper must guess from the shooter's body.",
  hint: "",
  arena: false,
  inputTicks: 2,

  setup(match) {
    const [halfGoal, reach, runUp, wide, range] = Array.from(match.layout());
    layout = { halfGoal, reach, runUp, wide };
    kickRange = range;
    cursor = 0;
    fire = false;
    colors = {
      you: css("--you") || colors.you,
      bot: css("--bot") || colors.bot,
      ink: css("--ink") || colors.ink,
      line: colors.line,
      grass: colors.grass,
    };
  },

  resize(w, h) {
    width = w;
    height = h;
    scale = Math.min(w / (2 * (layout.halfGoal + layout.wide) + 4), h / 12);
  },

  draw(scene: Scene) {
    const { ctx, match, debug, watching } = scene;
    ctx.clearRect(0, 0, width, height);
    const grass = ctx.createLinearGradient(0, goalY(), 0, height);
    grass.addColorStop(0, "#0b2219");
    grass.addColorStop(1, "#06140f");
    ctx.fillStyle = grass;
    ctx.fillRect(0, goalY() - scale * 2, width, height);
    const v = Array.from(match.view());
    const [shooter, phase, dist, lean, ballY, flight, keeperY, diveTarget, diving, , lastGoal, aim] = v;
    const keeper = 1 - shooter;
    const seatColor = (seat: number) => {
      if (debug && (watching || seat === 1)) {
        return intentColor(match.intent_of(seat));
      }
      return seat === 0 ? colors.you : colors.bot;
    };
    const gx0 = toX(-layout.halfGoal);
    const gx1 = toX(layout.halfGoal);
    ctx.strokeStyle = colors.ink;
    ctx.lineWidth = 4;
    ctx.beginPath();
    ctx.moveTo(gx0, goalY());
    ctx.lineTo(gx0, goalY() - scale * 2.2);
    ctx.lineTo(gx1, goalY() - scale * 2.2);
    ctx.lineTo(gx1, goalY());
    ctx.stroke();
    ctx.strokeStyle = colors.line;
    ctx.lineWidth = 1;
    for (let i = 1; i < 12; i++) {
      const x = gx0 + ((gx1 - gx0) * i) / 12;
      ctx.beginPath();
      ctx.moveTo(x, goalY());
      ctx.lineTo(x, goalY() - scale * 2.2);
      ctx.stroke();
    }
    ctx.beginPath();
    ctx.moveTo(0, goalY());
    ctx.lineTo(width, goalY());
    ctx.stroke();
    ctx.fillStyle = "rgba(255,255,255,0.6)";
    ctx.beginPath();
    ctx.arc(centreX(), spotY(), 3, 0, Math.PI * 2);
    ctx.fill();

    const humanSeat = watching ? -1 : 0;
    if (humanSeat === keeper || (humanSeat === shooter && phase < 2)) {
      ctx.strokeStyle = "rgba(255,255,255,0.35)";
      ctx.setLineDash([6, 6]);
      ctx.beginPath();
      ctx.moveTo(toX(cursor), goalY() - scale * 2.2);
      ctx.lineTo(toX(cursor), humanSeat === keeper ? goalY() + 6 : spotY());
      ctx.stroke();
      ctx.setLineDash([]);
    }

    keeperShape(ctx, toX(keeperY), goalY(), Math.sign(diveTarget - keeperY), diving > 0, seatColor(keeper));
    const ballDepth = goalY() + (spotY() - goalY()) * (1 - flight);
    const shooterY = spotY() + (dist / Math.max(layout.runUp, 1e-3)) * height * 0.22 + scale * 0.6;
    figure(ctx, centreX() + lean * scale * 0.5 - scale * 0.4, shooterY, lean * 0.45, scale * 1.2, seatColor(shooter));
    ctx.fillStyle = "#ffffff";
    ctx.shadowColor = "rgba(255,255,255,0.7)";
    ctx.shadowBlur = 10;
    ctx.beginPath();
    ctx.arc(toX(phase === 2 || phase === 3 ? ballY : 0), ballDepth, Math.max(5, scale * 0.22), 0, Math.PI * 2);
    ctx.fill();
    ctx.shadowBlur = 0;

    const plain = (seat: number) => (seat === 0 ? colors.you : colors.bot);
    if (phase === 3 && lastGoal >= 0) {
      const scorerColor = lastGoal === 1 ? plain(shooter) : plain(keeper);
      banner(ctx, lastGoal === 1 ? "Goal!" : "Saved", scorerColor);
    } else if (scene.countdown) {
      banner(ctx, scene.countdown, colors.ink);
    } else if (!watching && phase < 2) {
      ctx.fillStyle = colors.ink;
      ctx.font = `600 ${Math.max(14, scale * 0.5)}px system-ui, sans-serif`;
      ctx.textAlign = "center";
      ctx.fillText(
        shooter === 0 ? "You shoot: aim with the mouse, click when you reach the ball" : "You keep: move to guard, click to dive",
        centreX(),
        height - 28,
      );
    }
    if (debug) {
      for (const seat of watching ? [0, 1] : [1]) {
        const insight = insightOf(match, seat);
        const color = intentColor(insight.intent);
        for (const [cls, filled] of [
          [insight.shown, false],
          [insight.real, true],
        ] as const) {
          if (cls < 0) {
            continue;
          }
          const y = match.class_point(seat, cls)[1];
          ctx.strokeStyle = color;
          ctx.fillStyle = color;
          ctx.lineWidth = 2;
          if (filled) {
            ctx.fillRect(toX(y) - 6, goalY() - scale - 6, 12, 12);
          } else {
            ctx.strokeRect(toX(y) - 8, goalY() - scale - 8, 16, 16);
          }
        }
      }
    }
    void aim;
  },

  debugLines(match, watching: Pair | null) {
    if (!watching) {
      return [null, [LEGEND, ...describe(insightOf(match, 1))]];
    }
    const lines = [0, 1].map((seat) =>
      describeBot(`${seat === 0 ? "left" : "right"} · level ${watching[seat]}`, insightOf(match, seat), classNames(), classNames()),
    );
    return [[LEGEND, ...lines[0]], lines[1]];
  },

  press(event, rect) {
    cursor = Math.max(-(layout.halfGoal + layout.wide), Math.min(layout.halfGoal + layout.wide, (event.clientX - rect.left - centreX()) / scale));
    fire = true;
    return false;
  },

  move(event, rect) {
    cursor = Math.max(-(layout.halfGoal + layout.wide), Math.min(layout.halfGoal + layout.wide, (event.clientX - rect.left - centreX()) / scale));
  },

  release() {
    return false;
  },

  send(match) {
    const v = match.view();
    const [shooter, phase, dist] = v;
    const shooting = shooter === 0;
    const lean = Math.max(-1, Math.min(1, cursor / layout.halfGoal));
    const first = shooting ? lean : cursor;
    match.set_input(new Float32Array([first, 1, 0, cursor]));
    const legal = shooting ? phase === 1 || (phase === 0 && dist <= kickRange) : phase !== 3 && phase !== 4;
    if (fire && phase === 3) {
      fire = false;
    }
    if (fire && legal) {
      fire = false;
      match.push_input(new Float32Array([first, 1, 1, cursor]));
    }
  },

  sideColors() {
    return [colors.you, colors.bot];
  },
};
