import { readFileSync } from "node:fs";

const { instance } = await WebAssembly.instantiate(readFileSync(process.argv[2]), {});
const digest = BigInt.asUintN(64, instance.exports.run_search_digest());
let slowest = 0;
for (const level of [6, 7, 8]) {
  const t0 = performance.now();
  const n = Number(instance.exports.run_search_decisions(level));
  slowest = Math.max(slowest, (performance.now() - t0) / Math.max(n, 1));
}
process.stdout.write(`${digest} ${Math.round(slowest)}\n`);
