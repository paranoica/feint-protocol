import { readFileSync } from "node:fs";

const { instance } = await WebAssembly.instantiate(readFileSync(process.argv[2]), {});
const outputs = BigInt.asUintN(64, instance.exports.run_outputs_digest());
const match = BigInt.asUintN(64, instance.exports.run_match_digest());
process.stdout.write(`${outputs} ${match}\n`);
