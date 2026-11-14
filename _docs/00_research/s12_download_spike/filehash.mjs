// File hash (rollup of 2 MiB BLAKE3 block hashes) of a file on disk, with the sync-core-wasm package in Node.
// Usage: node filehash.mjs <pkg dir> <file...>; prints "<hex>  <file>".
import { openSync, readSync, closeSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
const [pkgDir, ...files] = process.argv.slice(2);
const wasm = await import(pathToFileURL(join(pkgDir, "sync_core_wasm.js")).href);
wasm.initSync({ module: readFileSync(join(pkgDir, "sync_core_wasm_bg.wasm")) });
const B = wasm.block_size();
export function fileHash(path) {
  const fd = openSync(path, "r");
  const buf = Buffer.allocUnsafe(B);
  const hashes = [];
  for (;;) {
    let n = 0;
    while (n < B) { const r = readSync(fd, buf, n, B - n, null); if (r === 0) break; n += r; }
    if (n === 0) break;
    hashes.push(Buffer.from(wasm.hash_block(buf.subarray(0, n))));
    if (n < B) break;
  }
  closeSync(fd);
  return Buffer.from(wasm.rollup(Buffer.concat(hashes))).toString("hex");
}
for (const f of files) console.log(`${fileHash(f)}  ${f}`);
