// Node leg of the vector gate: loads the wasm-pack `--target web` package and runs the shared vectors.
// Usage: node node-runner.mjs <pkg dir> <vector file>. Prints `LEG node PASS|FAIL`; exit 1 on FAIL.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { runVectors } from "./run-vectors.mjs";

const [pkgDir, vectorPath] = process.argv.slice(2);
if (!pkgDir || !vectorPath) {
  console.error("usage: node node-runner.mjs <pkg dir> <vector file>");
  process.exit(2);
}

const wasm = await import(pathToFileURL(join(pkgDir, "sync_core_wasm.js")).href);
const exports = wasm.initSync({ module: readFileSync(join(pkgDir, "sync_core_wasm_bg.wasm")) });
const started = Date.now();
const result = runVectors(wasm, exports.memory, readFileSync(vectorPath, "utf8"));

for (const diff of result.diffs) console.log(`DIFF node ${diff}`);
console.log(
  `[node] ${result.cases} cases in ${Date.now() - started} ms; memory after 6 MiB ${result.memory.afterSixMib}, ` +
    `after 100 MiB ${result.memory.afterHundredMib}, growth ${result.memory.growth}`,
);
console.log(`LEG node ${result.diffs.length === 0 ? "PASS" : `FAIL(${result.diffs.length} differences)`}`);
process.exit(result.diffs.length === 0 ? 0 : 1);
