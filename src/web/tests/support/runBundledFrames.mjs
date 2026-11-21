// Runs the decoder of a built (minified) bundle over the vectors given on stdin as {name: hex} and prints the
// results as JSON (bigint as a decimal string). Used by tests/bundle.test.ts.
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

const [bundle] = process.argv.slice(2);
const { decodeServerFrame, decodeClientFrame, encodeSubscribe, encodePing } = await import(
  pathToFileURL(bundle).href
);
const vectors = JSON.parse(readFileSync(0, 'utf8'));
const bytes = (hex) => Uint8Array.from(hex.match(/../g) ?? [], (b) => parseInt(b, 16));
const text = (value) =>
  JSON.stringify(value, (_k, v) => (typeof v === 'bigint' ? v.toString() : v));
const hex = (raw) => Array.from(raw, (b) => b.toString(16).padStart(2, '0')).join('');
const out = { server: {}, client: {}, encoded: {} };
for (const [name, h] of Object.entries(vectors)) {
  out.server[name] = text(decodeServerFrame(bytes(h)));
  out.client[name] = text(decodeClientFrame(bytes(h)));
}
out.encoded.subscribe = hex(encodeSubscribe(1n, 42n));
out.encoded.ping = hex(encodePing(7));
console.log(JSON.stringify(out));
