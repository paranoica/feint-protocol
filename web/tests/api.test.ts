import { test } from "node:test";
import assert from "node:assert/strict";
import { localSeed, profilePath, profileVersion, Saves, seedRequest, ticketFrom, uploadPath } from "../src/api.ts";

test("an upload names nick, ticket and the profile it played with", () => {
  assert.equal(uploadPath("ann b", "00ff", null), "/api/matches?nick=ann%20b&ticket=00ff");
  assert.equal(uploadPath("ёж&co", "00ff", 3), "/api/matches?nick=%D1%91%D0%B6%26co&ticket=00ff&profile=3");
  assert.equal(profilePath("a/b", "airhockey"), "/api/profile?nick=a%2Fb&game=airhockey");
  assert.equal(profilePath("ann", "pen alty"), "/api/profile?nick=ann&game=pen%20alty");
});

test("only a well-formed ticket is trusted", () => {
  const good = { ticket: "0123456789abcdef0123456789abcdef0", seed: 42 };
  assert.deepEqual(ticketFrom(good), good);
  assert.equal(ticketFrom(null), null);
  assert.equal(ticketFrom({ ticket: "zz", seed: 1 }), null);
  assert.equal(ticketFrom({ ticket: good.ticket, seed: -1 }), null);
  assert.equal(ticketFrom({ ticket: good.ticket, seed: 2 ** 32 }), null);
  assert.equal(ticketFrom({ ticket: good.ticket, seed: 1.5 }), null);
  assert.equal(ticketFrom({ ticket: good.ticket }), null);
});

test("profile versions and local seeds stay in range", () => {
  assert.equal(profileVersion("7"), 7);
  assert.equal(profileVersion(null), null);
  assert.equal(profileVersion("0"), null);
  assert.equal(profileVersion("-1"), null);
  assert.equal(profileVersion("1e3"), null);
  assert.equal(localSeed(() => 0), 0);
  assert.equal(localSeed(() => 0.999999999999), 0xffffffff);
});

test("a ticket is asked for with a POST that a plain page cannot forge", () => {
  const init = seedRequest();
  assert.equal(init.method, "POST");
  assert.deepEqual(init.headers, { "x-feint": "1" });
});

test("a rematch waits for the previous save, but never longer than the cap", async () => {
  const saves = new Saves();
  assert.equal(await saves.settled(50), true, "nothing pending");
  let finish: () => void = () => {};
  const upload = new Promise<void>((resolve) => {
    finish = resolve;
  });
  saves.track(upload);
  let done = false;
  const waiting = saves.settled(1000).then((ok) => {
    done = true;
    return ok;
  });
  await new Promise((r) => setTimeout(r, 20));
  assert.equal(done, false, "still saving");
  finish();
  assert.equal(await waiting, true);

  saves.track(Promise.reject(new Error("arena down")));
  assert.equal(await saves.settled(50), true, "a failed save does not block");

  saves.track(new Promise(() => {}));
  const started = Date.now();
  assert.equal(await saves.settled(30), false, "a hung save gives up at the cap");
  assert.ok(Date.now() - started < 500);
});
