// One origin for the SPA and the API, standing in for Caddy in the acceptance stack: static files with the
// SPA fallback and the CSP of deploy/compose/Caddyfile.mvp, /api/* proxied (streaming, Host kept), and the
// WebSocket upgrade of /api/v1/realtime relayed. Loopback only; it exists for acceptance runs, not for use.
import { existsSync, statSync } from "node:fs";
import { join, normalize, sep } from "node:path";

const port = Number(requiredEnv("WEB_PORT"));
const apiUrl = new URL(requiredEnv("API_URL"));
const webDist = requiredEnv("WEB_DIST");

const CSP =
  "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self' blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";
const HOP_BY_HOP = ["connection", "keep-alive", "proxy-authenticate", "proxy-authorization", "te", "trailer", "transfer-encoding", "upgrade", "host"];
const REALTIME = "/api/v1/realtime";

function requiredEnv(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is required`);
  return value;
}

function staticFile(pathname: string): Response {
  const root = normalize(webDist);
  const candidate = normalize(join(root, decodeURIComponent(pathname)));
  const inside = candidate === root || candidate.startsWith(root + sep);
  const isFile = inside && existsSync(candidate) && statSync(candidate).isFile();
  // An unknown path gets index.html and the client router decides, as in the Caddyfile.
  const file = Bun.file(isFile ? candidate : join(root, "index.html"));
  return new Response(file, { headers: { "content-security-policy": CSP, "cache-control": "no-cache" } });
}

function upstreamHeaders(request: Request, clientIp: string): Headers {
  const headers = new Headers();
  for (const [name, value] of request.headers) if (!HOP_BY_HOP.includes(name)) headers.set(name, value);
  headers.set("host", new URL(request.url).host);
  headers.set("x-forwarded-for", clientIp);
  return headers;
}

async function proxy(request: Request, clientIp: string): Promise<Response> {
  const url = new URL(request.url);
  const target = new URL(url.pathname + url.search, apiUrl);
  try {
    const upstream = await fetch(target, {
      method: request.method,
      headers: upstreamHeaders(request, clientIp),
      body: request.body,
      redirect: "manual",
      duplex: "half",
    });
    const headers = new Headers(upstream.headers);
    for (const name of HOP_BY_HOP) headers.delete(name);
    return new Response(upstream.body, { status: upstream.status, headers });
  } catch (error) {
    console.error(`proxy ${request.method} ${url.pathname} failed:`, error);
    return new Response("acceptance proxy: upstream unreachable\n", { status: 502 });
  }
}

/** Opens the upstream socket first, so the browser's upgrade is answered only when the API accepted it. */
function openUpstream(request: Request, clientIp: string): Promise<WebSocket> {
  const url = new URL(request.url);
  const target = new URL(url.pathname, apiUrl);
  target.protocol = "ws:";
  const headers: Record<string, string> = {};
  for (const [name, value] of upstreamHeaders(request, clientIp)) {
    if (!name.startsWith("sec-websocket-")) headers[name] = value;
  }
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(target, { headers });
    socket.binaryType = "arraybuffer";
    socket.onopen = () => resolve(socket);
    socket.onerror = () => reject(new Error("the API refused the WebSocket handshake"));
  });
}

function closeCode(code: number): number | undefined {
  return code === 1005 || code === 1006 || code === 1015 ? undefined : code;
}

interface SocketData {
  upstream: WebSocket;
}

const server = Bun.serve<SocketData>({
  hostname: "127.0.0.1",
  port,
  maxRequestBodySize: 4 * 1024 ** 3,
  idleTimeout: 120,
  async fetch(request, server) {
    const { pathname } = new URL(request.url);
    const clientIp = server.requestIP(request)?.address ?? "127.0.0.1";
    if (pathname === "/__acceptance/health") return new Response("ok\n");
    if (pathname === REALTIME && request.headers.get("upgrade")?.toLowerCase() === "websocket") {
      let upstream: WebSocket;
      try {
        upstream = await openUpstream(request, clientIp);
      } catch (error) {
        console.error("realtime upstream failed:", error);
        return new Response("acceptance proxy: upstream refused the WebSocket\n", { status: 502 });
      }
      if (server.upgrade(request, { data: { upstream } })) return undefined;
      upstream.close();
      return new Response("upgrade failed\n", { status: 400 });
    }
    if (pathname.startsWith("/api/")) return proxy(request, clientIp);
    return staticFile(pathname);
  },
  websocket: {
    open(ws) {
      const { upstream } = ws.data;
      upstream.onmessage = (event) => ws.send(event.data);
      upstream.onclose = (event) => ws.close(closeCode(event.code), event.reason);
    },
    message(ws, message) {
      ws.data.upstream.send(message);
    },
    close(ws, code, reason) {
      ws.data.upstream.close(closeCode(code), reason);
    },
  },
});

console.log(`acceptance web server on http://127.0.0.1:${server.port}, api ${apiUrl.origin}, dist ${webDist}`);
for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.on(signal, () => {
    void server.stop(true);
    process.exit(0);
  });
}
