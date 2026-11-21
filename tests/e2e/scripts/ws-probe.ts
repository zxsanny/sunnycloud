// Opens /api/v1/realtime through the acceptance web server with a session cookie and prints the first frame:
// proves the WebSocket upgrade passes the proxy. Usage: ACC_COOKIE='__Host-session=...' bun ws-probe.ts <origin> [expect-refused]
export {};
const siteOrigin = process.argv[2];
const expectRefused = process.argv[3] === "expect-refused";
if (!siteOrigin) throw new Error("usage: ws-probe.ts <origin> [expect-refused]");
const cookie = process.env.ACC_COOKIE ?? "";
const url = new URL("/api/v1/realtime", siteOrigin);
url.protocol = "ws:";

const socket = new WebSocket(url, { headers: { origin: siteOrigin, ...(cookie ? { cookie } : {}) } });
socket.binaryType = "arraybuffer";
const outcome = await new Promise<string>((resolve) => {
  const timer = setTimeout(() => resolve("timeout"), 8000);
  socket.onmessage = (event) => {
    clearTimeout(timer);
    const bytes = new Uint8Array(event.data as ArrayBuffer);
    resolve(`frame type byte ${bytes[0]}, ${bytes.length} bytes`);
  };
  socket.onerror = () => {
    clearTimeout(timer);
    resolve("refused");
  };
});
socket.close();
console.log(`realtime: ${outcome}`);
const ok = expectRefused ? outcome === "refused" : outcome.startsWith("frame");
process.exit(ok ? 0 : 1);
