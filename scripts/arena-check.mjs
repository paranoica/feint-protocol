import { readFileSync } from "node:fs";

const [base, glue, wasm] = process.argv.slice(2);
const { initSync, WebMatch } = await import(glue);
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
process.stdout.write(`matches ${one.id} ${two.id} profiles ${one.profile} ${two.profile}\n`);
