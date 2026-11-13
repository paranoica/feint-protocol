import { readFileSync, writeFileSync } from "node:fs";
import { performance } from "node:perf_hooks";

const [glue, wasm, out, trainedOut, fakesJson, cardOut] = process.argv.slice(2);
const { initSync, WebMatch, checkpoints, WebCards, card_games, card_checkpoints } = await import(glue);
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

function trainedMatch(level, seed) {
  const match = WebMatch.create_trained("airhockey", level, seed, levels[level - 1]);
  while (!match.finished() && match.tick() < 240 * 600) {
    const p = match.positions();
    match.set_pointer(p[0], p[1]);
    match.advance(4);
  }
  return match.record();
}

for (const level of [1, 5]) {
  const first = trainedMatch(level, 0x1e7e1 + level);
  const second = trainedMatch(level, 0x1e7e1 + level);
  if (first.length !== second.length || !first.every((b, i) => b === second[i])) {
    throw new Error(`trained level ${level} plays differently with one seed`);
  }
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

if (fakesJson) {
  const fakes = JSON.parse(readFileSync(fakesJson, "utf8"));
  if (fakes.checkpoint !== checkpoints()[fakes.level - 1]) {
    throw new Error(`fakes.json measured ${fakes.checkpoint}, level ${fakes.level} ships ${checkpoints()[fakes.level - 1]}`);
  }
  const link = fakes.links[0];
  if (!link) {
    process.stdout.write("none\n");
  } else {
    const rerun = WebMatch.rerun("trained", fakes.level, link.seed, link.seat);
    if (!rerun.spectating() || rerun.record().length !== 0) {
      throw new Error("a rerun must be watched and leave no record");
    }
    while (!rerun.finished()) {
      rerun.advance(4800);
    }
    process.stdout.write(`${rerun.digest_hex()} ${fakes.level} ${link.seed} ${link.seat}\n`);
  }
}

if (cardOut) {
  if (card_games().join(",") !== "believe" || card_checkpoints().length !== 1) {
    throw new Error(`card games or checkpoints are not listed: ${card_games()} ${card_checkpoints()}`);
  }
  function cardMatch(seed) {
    const match = new WebCards(seed, card_checkpoints()[0]);
    let ticks = 0;
    let moves = 0;
    while (!match.finished() && ticks < 36000) {
      if (match.my_turn()) {
        const legal = match.legal();
        const pick = legal[(moves * 7 + seed) % legal.length];
        const move = match.describe(pick);
        if (move.length !== 1 && move.length !== 1 + match.ranks()) {
          throw new Error(`move ${pick} describes as ${move}`);
        }
        if (!match.play(pick) || match.play(pick)) {
          throw new Error("a legal move is queued once");
        }
        moves += 1;
      }
      match.advance(1);
      ticks += 1;
    }
    if (!match.finished() || match.outcome() === undefined || match.my_turn() || match.legal().length !== 0) {
      throw new Error("the card match did not end cleanly");
    }
    if (match.events().length % match.event_stride() !== 0 || match.events().length === 0) {
      throw new Error("card events are not whole strides");
    }
    return match;
  }
  const first = cardMatch(0xca7d);
  const again = cardMatch(0xca7d).record();
  const record = first.record();
  if (record.length !== again.length || !record.every((b, i) => b === again[i])) {
    throw new Error("a card match plays differently with one seed");
  }
  writeFileSync(cardOut, record);
  process.stdout.write(`${first.digest_hex()}\n`);
}
