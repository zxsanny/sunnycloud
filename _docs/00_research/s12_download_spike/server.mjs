// Static server for the S12 spike: every response carries exactly the 16 §5 CSP.
// Routes: / and /*.js|.html from www, /pkg/* from www/pkg, /files/<name>[?rate=MiBps] streamed from data.
import { createReadStream, statSync, readFileSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, normalize } from "node:path";
const [www, data, port] = process.argv.slice(2);
export const CSP = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self' blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";
const TYPES = { ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".wasm": "application/wasm", ".bin": "application/octet-stream" };
const reports = {};
const link = { next: 0 };
createServer((req, res) => {
  const u = new URL(req.url, "http://127.0.0.1");
  const headers = { "content-security-policy": CSP, "cache-control": "no-store" };
  let path = u.pathname === "/" ? "/index.html" : u.pathname;
  const rel = normalize(path).replace(/^\/+/, "");
  if (rel.startsWith("..")) { res.writeHead(404, headers); return res.end(); }
  if (rel === "report") {
    if (req.method === "POST") {
      let body = ""; req.on("data", (c) => (body += c)); req.on("end", () => {
        const d = JSON.parse(body); (reports[d.run] ??= []).push(d); console.log(`[server] report ${body.slice(0, 400)}`);
        res.writeHead(204, headers); res.end();
      });
      return;
    }
    const got = reports[u.searchParams.get("run")];
    res.writeHead(got ? 200 : 404, { ...headers, "content-type": "application/json" }); return res.end(JSON.stringify(got ?? null));
  }
  if (rel.startsWith("files/")) {
    const file = join(data, rel.slice(6));
    let size;
    try { size = statSync(file).size; } catch { res.writeHead(404, headers); return res.end(); }
    const m = /^bytes=(\d+)-(\d*)$/.exec(req.headers.range ?? "");
    let start = 0, end = size - 1;
    if (m) {
      start = Number(m[1]); end = m[2] ? Math.min(Number(m[2]), size - 1) : size - 1;
      if (start >= size) { res.writeHead(416, { ...headers, "content-range": `bytes */${size}` }); return res.end(); }
      res.writeHead(206, { ...headers, "content-type": TYPES[".bin"], "content-length": end - start + 1, "content-range": `bytes ${start}-${end}/${size}`, "accept-ranges": "bytes" });
    } else res.writeHead(200, { ...headers, "content-type": TYPES[".bin"], "content-length": size, "accept-ranges": "bytes" });
    const rate = Number(u.searchParams.get("rate") || 0) * 1048576; // bytes per second over all responses, 0 = unthrottled
    const stream = createReadStream(file, { start, end, highWaterMark: 256 * 1024 });
    if (!rate) { stream.pipe(res); }
    else {
      // One shared budget for all concurrent responses (a single link of `rate` bytes/s).
      stream.on("data", (chunk) => {
        const now = Date.now(); link.next = Math.max(link.next, now) + chunk.length / rate * 1000;
        res.write(chunk);
        const wait = link.next - now;
        if (wait > 0) { stream.pause(); setTimeout(() => stream.resume(), wait); }
      });
      stream.on("end", () => res.end());
    }
    res.on("close", () => { stream.destroy(); if (!m || !res.writableFinished) console.log(`[server] ${req.url} closed after ${res.writableFinished ? "finish" : "abort"}`); });
    if (!m) console.log(`[server] GET ${req.url} ${size} B`);
    return;
  }
  try {
    const body = readFileSync(join(www, rel));
    res.writeHead(200, { ...headers, "content-type": TYPES[extname(rel)] ?? "application/octet-stream" });
    res.end(body);
  } catch { res.writeHead(404, headers); res.end(); }
}).listen(Number(port), "127.0.0.1", () => console.log(`[server] listening ${port}`));
