import { readFileSync } from "node:fs";

const { instance } = await WebAssembly.instantiate(readFileSync(process.argv[2]), {});
const kuhn = BigInt.asUintN(64, instance.exports.run_kuhn_digest());
const believe = BigInt.asUintN(64, instance.exports.run_believe_digest());
process.stdout.write(`${kuhn} ${believe}\n`);
