import { readFileSync } from 'node:fs';
import { join } from 'node:path';

/** The shared vector file of the realtime frames (`name hex` lines; `-` is the empty message). */
export function loadVectors(): Map<string, Uint8Array> {
  const path = join(process.cwd(), '../components/11_http_api/tests/fixtures/realtime_frames.txt');
  const vectors = new Map<string, Uint8Array>();
  for (const line of readFileSync(path, 'utf8').split('\n')) {
    if (line.trim() === '' || line.startsWith('#')) continue;
    const [name, hex] = line.split(' ');
    if (!name || hex === undefined) throw new Error(`malformed vector line: ${line}`);
    vectors.set(
      name,
      hex === '-'
        ? new Uint8Array()
        : Uint8Array.from(hex.match(/../g) ?? [], (b) => parseInt(b, 16)),
    );
  }
  return vectors;
}

export const hexOf = (bytes: Uint8Array) =>
  Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
