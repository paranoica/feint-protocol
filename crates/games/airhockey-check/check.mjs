import { readFileSync } from "node:fs";

const { instance } = await WebAssembly.instantiate(readFileSync(process.argv[2]), {});
process.stdout.write(`${BigInt.asUintN(64, instance.exports.run_digest())}\n`);
