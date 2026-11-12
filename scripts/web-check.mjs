import { readFileSync, writeFileSync } from "node:fs";
import { performance } from "node:perf_hooks";

const [glue, wasm, out, trainedOut] = process.argv.slice(2);
const { initSync, WebMatch, checkpoints } = await import(glue);
initSync({ module: readFileSync(wasm) });

const game = new WebMatch(5, 0x5eed);
const limit = 240 * 120;
const start = performance.now();
while (!game.finished() && game.tick() < limit) {
  const p = game.positions();
  game.set_pointer(p[0], p[1]);
  game.advance(4);
}
const seconds = (performance.now() - start) / 1000;
writeFileSync(out, game.record());
process.stdout.write(`${game.digest_hex()} ${Math.round(game.tick() / seconds)}\n`);

const levels = checkpoints();
if (levels.length !== 5 || levels[1] !== levels[2] || levels[2] !== levels[3] || levels[0] === levels[1]) {
  throw new Error(`checkpoints are not listed one per level: ${levels.join(",")}`);
}

if (trainedOut) {
  const trained = WebMatch.create_trained("airhockey", 5, 0x7ea1, checkpoints()[4]);
  while (!trained.finished() && trained.tick() < 240 * 60) {
    const p = trained.positions();
    trained.set_pointer(p[0], p[1]);
    trained.advance(4);
  }
  writeFileSync(trainedOut, trained.record());
  process.stdout.write(`${trained.digest_hex()}\n`);
}
