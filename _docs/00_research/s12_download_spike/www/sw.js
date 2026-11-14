// Candidate B: module Service Worker streams the download to the browser's download manager. Each 2 MiB block is
// hashed with the WASM hasher on the way through; the last block is held back until the rollup equals the expected
// hash, so a mismatch errors the stream and the download never completes.
import init, { hash_block, rollup, block_size } from "/pkg/sync_core_wasm.js";
import { hex, concat, Blocker, rangedSource } from "/common.js";
const ready = init({ module_or_path: "/pkg/sync_core_wasm_bg.wasm" });
const bc = new BroadcastChannel("s12");
self.addEventListener("securitypolicyviolation", (e) => bc.postMessage({ violation: `sw: ${e.violatedDirective} ${e.blockedURI}` }));
self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (e) => e.waitUntil(self.clients.claim()));
self.addEventListener("message", (e) => { if (e.data === "ping") bc.postMessage({ pong: true }); });
self.addEventListener("fetch", (event) => {
  const u = new URL(event.request.url);
  if (u.pathname !== "/dl") return;
  event.respondWith(stream(u));
});
async function stream(u) {
  await ready;
  const id = u.searchParams.get("id"), expected = u.searchParams.get("h"), name = u.searchParams.get("name");
  const t0 = performance.now();
  const src = u.searchParams.get("src");
  const ranged = u.searchParams.get("mode") === "range";
  const res = ranged ? null : await fetch(src);
  const hashes = []; let held = null, bytes = 0, hashMs = 0;
  let out;
  const blocker = new Blocker(block_size(), (block, len) => {
    const view = len === block.length ? block : block.subarray(0, len);
    const t = performance.now(); hashes.push(hash_block(view)); hashMs += performance.now() - t;
    bytes += len;
    if (held) out.enqueue(held);
    held = view;
    return new Uint8Array(block.length); // the held block is handed to the stream later; never reuse it
  });
  const ts = new TransformStream({
    transform(chunk, c) { out = c; blocker.push(chunk); },
    flush(c) {
      out = c; blocker.end();
      const fileHash = hex(rollup(concat(hashes)));
      const report = { id, fileHash, bytes, ms: performance.now() - t0, hashMs };
      if (fileHash === expected) { if (held) c.enqueue(held); bc.postMessage({ ...report, ok: true }); }
      else { bc.postMessage({ ...report, ok: false, reason: "HashMismatch" }); c.error(new Error("HashMismatch")); }
    },
  });
  let body;
  if (ranged) {
    // Pull-based source: a range is fetched only when the download manager pulls.
    const source = rangedSource(src);
    body = new ReadableStream({ async pull(c) { const { done, value } = await source.read(); if (done) c.close(); else c.enqueue(value); } }, { highWaterMark: 0 }).pipeThrough(ts);
  } else body = res.body.pipeThrough(ts);
  const headers = { "content-type": "application/octet-stream", "content-disposition": `attachment; filename="${name}"` };
  if (u.searchParams.get("size")) headers["content-length"] = u.searchParams.get("size");
  else if (res) headers["content-length"] = res.headers.get("content-length");
  return new Response(body, { headers });
}
