import { readFileSync, writeFileSync } from "node:fs";

const [base, glue, wasm, patchedOut] = process.argv.slice(2);
const { initSync, WebMatch, checkpoints, WebCards, card_checkpoints } = await import(glue);
initSync({ module: readFileSync(wasm) });

function play(game) {
  for (let i = 0; i < 240 * 60 * 15 && !game.finished(); i += 4) {
    const p = game.positions();
    game.set_pointer(p[0], p[1]);
    game.advance(4);
  }
  if (!game.finished()) {
    throw new Error("the match did not finish");
  }
  return game.record();
}

async function ticket() {
  const response = await fetch(`${base}/api/seed`, { method: "POST", headers: { "x-feint": "1" } });
  if (response.status !== 200) {
    throw new Error(`seed: ${response.status}`);
  }
  return response.json();
}

async function upload(record, t, profile) {
  const query = `nick=arena-check&ticket=${t.ticket}${profile ? `&profile=${profile}` : ""}`;
  const response = await fetch(`${base}/api/matches?${query}`, {
    method: "POST",
    headers: { "x-feint": "1" },
    body: record,
  });
  const body = await response.json();
  if (response.status !== 201) {
    throw new Error(`upload: ${response.status} ${JSON.stringify(body)}`);
  }
  return body;
}

const first = await ticket();
const one = await upload(play(new WebMatch(5, first.seed)), first, null);
if (one.profile !== 1) {
  throw new Error(`first match stored profile ${one.profile}`);
}
const got = await fetch(`${base}/api/profile?nick=arena-check`);
const version = got.headers.get("x-profile-version");
const bytes = new Uint8Array(await got.arrayBuffer());
const second = await ticket();
const two = await upload(play(WebMatch.new_with_profile(5, second.seed, bytes)), second, version);
if (two.profile !== 2) {
  throw new Error(`second match stored profile ${two.profile}`);
}

const checkpoint = checkpoints()[2];
const third = await ticket();
const trainedRecord = play(WebMatch.create_trained("airhockey", 3, third.seed, checkpoint));
const three = await upload(trainedRecord, third, null);
if (three.profile !== null) {
  throw new Error(`a trained match stored profile ${three.profile}`);
}

function idBytes(hex) {
  const bytes = new Uint8Array(8);
  new DataView(bytes.buffer).setBigUint64(0, BigInt(`0x${hex}`), true);
  return bytes;
}

function places(haystack, needle) {
  const found = [];
  for (let i = 0; i + needle.length <= haystack.length; i++) {
    if (needle.every((b, j) => haystack[i + j] === b)) {
      found.push(i);
    }
  }
  return found;
}

const record = new Uint8Array(trainedRecord);
const headerEnd = 4 + new DataView(record.buffer, record.byteOffset).getUint32(0, true);
const at = places(record.subarray(4, headerEnd), idBytes(checkpoint));
if (at.length !== 1) {
  throw new Error(`the checkpoint id occurs ${at.length} times in the header frame`);
}
const unknown = (BigInt(`0x${checkpoint}`) ^ 1n).toString(16).padStart(16, "0");
const patched = record.slice();
patched.set(idBytes(unknown), 4 + at[0]);
writeFileSync(patchedOut, patched);
const fourth = await ticket();
const refused = await fetch(`${base}/api/matches?nick=arena-check&ticket=${fourth.ticket}`, {
  method: "POST",
  headers: { "x-feint": "1" },
  body: patched,
});
const reason = await refused.json();
if (refused.status !== 400 || reason.error !== "record") {
  throw new Error(`an unknown checkpoint got ${refused.status} ${JSON.stringify(reason)}`);
}
const fifth = await ticket();
const cards = new WebCards(fifth.seed, card_checkpoints()[0]);
for (let i = 0; i < 36000 && !cards.finished(); i++) {
  if (cards.my_turn()) {
    const legal = cards.legal();
    cards.play(legal[i % legal.length]);
  }
  cards.advance(1);
}
if (!cards.finished()) {
  throw new Error("the card match did not finish");
}
const five = await upload(cards.record(), fifth, null);
if (five.profile !== null) {
  throw new Error(`a card match stored profile ${five.profile}`);
}
const results = await (await fetch(`${base}/api/results?nick=arena-check`)).json();
const row = results.find((r) => r.game === "believe");
if (!row || row.level !== 5 || row.matches !== 1) {
  throw new Error(`the card match is not in the results: ${JSON.stringify(results)}`);
}
process.stdout.write(
  `matches ${one.id} ${two.id} profiles ${one.profile} ${two.profile} trained ${three.id} cards ${five.id} unknown checkpoint refused\n`,
);
