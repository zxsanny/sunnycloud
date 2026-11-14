// Candidate A: dedicated module Worker. fetch -> 2 MiB blocks -> WASM hash_block -> OPFS sync access handle.
// The file reaches the user's Downloads only after the rollup equals the expected hash (page saves the OPFS File).
import init, { hash_block, rollup, block_size } from "/pkg/sync_core_wasm.js";
import { hex, concat, Blocker, rangedSource } from "/common.js";
const violations = [];
self.addEventListener("securitypolicyviolation", (e) => violations.push(`${e.violatedDirective} ${e.blockedURI}`));
self.onmessage = async ({ data: { src, expected, opfsName, mode } }) => {
  const t0 = performance.now();
  let root, access;
  try {
    await init({ module_or_path: "/pkg/sync_core_wasm_bg.wasm" });
    root = await navigator.storage.getDirectory();
    const fh = await root.getFileHandle(opfsName, { create: true });
    access = await fh.createSyncAccessHandle();
    access.truncate(0);
    let reader;
    if (mode === "range") reader = rangedSource(src);
    else { const res = await fetch(src); if (!res.ok) throw new Error(`HTTP ${res.status}`); reader = res.body.getReader(); }
    const hashes = []; let off = 0; let hashMs = 0, writeMs = 0;
    const blocker = new Blocker(block_size(), (block, len) => {
      const view = len === block.length ? block : block.subarray(0, len);
      let t = performance.now(); hashes.push(hash_block(view)); hashMs += performance.now() - t;
      t = performance.now();
      const w = access.write(view, { at: off });
      if (w !== len) throw new Error(`short write ${w}/${len}`);
      writeMs += performance.now() - t; off += len;
    });
    for (;;) { const { done, value } = await reader.read(); if (done) break; blocker.push(value); }
    blocker.end();
    access.flush(); access.close(); access = null;
    const fileHash = hex(rollup(concat(hashes)));
    if (fileHash !== expected) {
      await root.removeEntry(opfsName);
      postMessage({ ok: false, reason: "HashMismatch", fileHash, bytes: off, ms: performance.now() - t0, hashMs, writeMs, violations });
      return;
    }
    postMessage({ ok: true, fileHash, bytes: off, ms: performance.now() - t0, hashMs, writeMs, violations });
  } catch (error) {
    try { access?.close(); } catch (e) { violations.push(`close failed: ${e}`); }
    postMessage({ ok: false, reason: `${error?.name}: ${error?.message}`, ms: performance.now() - t0, violations });
  }
};
