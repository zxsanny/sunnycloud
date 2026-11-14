// Negative control (not a candidate): whole file into one ArrayBuffer in the Worker, then hashed block by block.
import init, { hash_block, rollup, block_size } from "/pkg/sync_core_wasm.js";
import { hex, concat } from "/common.js";
self.onmessage = async ({ data: { src, expected } }) => {
  const t0 = performance.now();
  try {
    await init({ module_or_path: "/pkg/sync_core_wasm_bg.wasm" });
    const buf = new Uint8Array(await (await fetch(src)).arrayBuffer());
    const B = block_size(); const hashes = [];
    for (let o = 0; o < buf.length; o += B) hashes.push(hash_block(buf.subarray(o, Math.min(o + B, buf.length))));
    const fileHash = hex(rollup(concat(hashes)));
    postMessage({ ok: fileHash === expected, reason: fileHash === expected ? "" : "HashMismatch", fileHash, bytes: buf.length, ms: performance.now() - t0, violations: [] });
  } catch (error) { postMessage({ ok: false, reason: `${error?.name}: ${error?.message}`, violations: [] }); }
};
