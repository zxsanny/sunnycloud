// Browser leg, inside a module Worker as in production (16): loads the package and runs the vectors.
import init, * as wasm from "/pkg/sync_core_wasm.js";
import { runVectors } from "/js/run-vectors.mjs";

try {
  const exports = await init({ module_or_path: "/pkg/sync_core_wasm_bg.wasm" });
  const text = await (await fetch("/vectors.txt")).text();
  const started = performance.now();
  const result = runVectors(wasm, exports.memory, text);
  postMessage({ ...result, ms: Math.round(performance.now() - started) });
} catch (error) {
  postMessage({ error: `${error?.name}: ${error?.message}` });
}
