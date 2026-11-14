// Shared by the Worker and the Service Worker: block-by-block hashing with the sync-core-wasm hasher.
export const hex = (bytes) => Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
export function concat(parts) {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let o = 0; for (const p of parts) { out.set(p, o); o += p.length; }
  return out;
}
// Feeds network chunks into fixed 2 MiB blocks; onBlock(block, length) gets each full block (and the last partial one at end()).
export class Blocker {
  constructor(size, onBlock) { this.size = size; this.onBlock = onBlock; this.block = new Uint8Array(size); this.fill = 0; }
  push(chunk) {
    let p = 0;
    while (p < chunk.length) {
      const n = Math.min(this.size - this.fill, chunk.length - p);
      this.block.set(chunk.subarray(p, p + n), this.fill); this.fill += n; p += n;
      if (this.fill === this.size) { const b = this.block; this.fill = 0; this.block = this.onBlock(b, this.size) ?? b; }
    }
  }
  end() { if (this.fill > 0) this.onBlock(this.block, this.fill); }
}
// Ranged pull: the next byte range is requested only when the consumer has drained the current one (at most 2
// ranges in flight), so the browser never buffers more than about 2 ranges whatever the link speed. Each range body
// is read as a stream (no per-range ArrayBuffer). Ranges are block-aligned.
export const RANGE = 8 * 1024 * 1024;
export function rangedSource(src) {
  let total = null, next = 0, current = null; const inflight = [];
  const get = (a) => fetch(src, { headers: { range: `bytes=${a}-${a + RANGE - 1}` }, cache: "no-store" }).then((r) => {
    if (r.status !== 206) throw new Error(`expected 206, got ${r.status}`);
    const t = Number(/\/(\d+)$/.exec(r.headers.get("content-range"))?.[1]);
    if (!Number.isFinite(t)) throw new Error("no total in Content-Range");
    if (total === null) total = t; else if (total !== t) throw new Error("file changed between ranges");
    return r.body.getReader();
  });
  const top = () => { while (inflight.length < 2 && (total === null ? next === 0 : next < total)) { inflight.push(get(next)); next += RANGE; } };
  return {
    async read() {
      for (;;) {
        if (!current) { top(); if (!inflight.length) return { done: true }; current = await inflight.shift(); top(); }
        const chunk = await current.read();
        if (!chunk.done) return chunk;
        current = null;
      }
    },
  };
}
