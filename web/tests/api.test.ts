import { test } from "node:test";
import assert from "node:assert/strict";
import { localSeed, profilePath, profileVersion, ticketFrom, uploadPath } from "../src/api.ts";

test("an upload names nick, ticket and the profile it played with", () => {
  assert.equal(uploadPath("ann b", "00ff", null), "/api/matches?nick=ann%20b&ticket=00ff");
  assert.equal(uploadPath("ёж&co", "00ff", 3), "/api/matches?nick=%D1%91%D0%B6%26co&ticket=00ff&profile=3");
  assert.equal(profilePath("a/b"), "/api/profile?nick=a%2Fb");
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
