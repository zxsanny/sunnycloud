import { fillTestBytes } from '../../../sync-core-wasm/js/run-vectors.mjs';

/** The generated file of the shared vectors: byte i is (i mod 251). */
export function patternBytes(length: number): Uint8Array<ArrayBuffer> {
  const bytes = new Uint8Array(length);
  fillTestBytes(bytes, 0);
  return bytes;
}

export function patternFile(length: number, name = 'pattern.bin'): File {
  return new File([patternBytes(length)], name, { lastModified: 1_700_000_000_000 });
}

export const MIB = 1024 * 1024;

/** Byte equality that stays fast for megabytes (a deep `toEqual` on typed arrays does not). */
export function sameBytes(a: Uint8Array | undefined, b: Uint8Array | undefined): boolean {
  return a !== undefined && b !== undefined && Buffer.compare(a, b) === 0;
}
