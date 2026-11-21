import { isTransientError, type Api, type ApiError } from '$lib/api';
import type { Failure } from './protocol';

export const hex = (bytes: Uint8Array) =>
  Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');

/** The 32 bytes of each hash, back to back, or null when the list is not `count` hashes of 64 hex digits. */
export function hashesOf(blocklist: string[], count: number): Uint8Array | null {
  if (!Array.isArray(blocklist) || blocklist.length !== count) return null;
  const bytes = new Uint8Array(count * 32);
  for (let i = 0; i < count; i += 1) {
    const text = blocklist[i];
    if (typeof text !== 'string' || !/^[0-9a-f]{64}$/.test(text)) return null;
    for (let b = 0; b < 32; b += 1) bytes[i * 32 + b] = parseInt(text.slice(b * 2, b * 2 + 2), 16);
  }
  return bytes;
}

export class Cancelled extends Error {}
export class Stopped extends Error {
  constructor(readonly failure: Failure) {
    super('transfer stopped');
  }
}

export function apiFailure(error: ApiError): Failure {
  return { kind: 'api', error };
}

/** Results of block sends, in the order they finish. */
export class Mailbox<T> {
  private items: T[] = [];
  private waiting: ((item: T) => void) | null = null;

  push(item: T) {
    if (this.waiting) {
      const wake = this.waiting;
      this.waiting = null;
      wake(item);
    } else {
      this.items.push(item);
    }
  }

  take(): Promise<T> {
    const item = this.items.shift();
    if (item !== undefined) return Promise.resolve(item);
    return new Promise((resolve) => {
      this.waiting = resolve;
    });
  }
}

export type BlockOutcome =
  | { kind: 'Acked' | 'HashRejected' | 'SessionGone' }
  | { kind: 'Transient' | 'BlockConflict'; retry_after_ms?: number };
export type Sent = { i: number; outcome: BlockOutcome } | { stop: Failure } | { cancelled: true };

/** Reads `[start, end)` of the file; a read that fails (file moved, deleted or changed) stops that job. */
export async function readSlice(file: Blob, start: number, end: number): Promise<Uint8Array> {
  try {
    return new Uint8Array(await file.slice(start, end).arrayBuffer());
  } catch (cause) {
    if (cause instanceof DOMException) {
      throw new Stopped({ kind: 'local', reason: 'FileUnreadable' });
    }
    throw cause;
  }
}

/** One block, one request, no streaming body. Never throws: the outcome goes to the mailbox. */
export async function sendBlock(
  api: Api,
  file: Blob,
  i: number,
  end: number,
  blockSize: number,
  uploadId: string,
  hashes: Uint8Array,
  signal: AbortSignal,
): Promise<Sent> {
  let bytes: Uint8Array;
  try {
    bytes = await readSlice(file, i * blockSize, end);
  } catch (cause) {
    if (cause instanceof Stopped) return { stop: cause.failure };
    throw cause;
  }
  const result = await api.putBlock(
    uploadId,
    i,
    bytes,
    hex(hashes.subarray(i * 32, (i + 1) * 32)),
    signal,
  );
  if (result.ok) return { i, outcome: { kind: 'Acked' } };
  if (signal.aborted) return { cancelled: true };
  const error = result.error;
  const retryAfterMs =
    error.retryAfterSeconds === null ? undefined : error.retryAfterSeconds * 1000;
  if (error.code === 'block_hash_mismatch' || error.code === 'block_length_mismatch') {
    return { i, outcome: { kind: 'HashRejected' } };
  }
  if (error.code === 'block_conflict') {
    return { i, outcome: { kind: 'BlockConflict', retry_after_ms: retryAfterMs } };
  }
  if (error.code === 'upload_session_gone') return { i, outcome: { kind: 'SessionGone' } };
  if (isTransientError(error))
    return { i, outcome: { kind: 'Transient', retry_after_ms: retryAfterMs } };
  return { stop: apiFailure(error) };
}

export function defaultSleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(done, ms);
    signal.addEventListener('abort', done, { once: true });
    function done() {
      clearTimeout(timer);
      signal.removeEventListener('abort', done);
      resolve();
    }
  });
}

/** What the file looked like when the upload began; a different value before commit means it was edited. */
export interface FileStamp {
  size: number;
  lastModified: number | undefined;
}

export const stampOf = (file: Blob): FileStamp => ({
  size: file.size,
  lastModified: (file as Partial<File>).lastModified,
});

/**
 * What a resumed upload may send a second time: the missing blocks that lie before the last block the server holds
 * (they were on the wire when the upload stopped), and the first `lanes` missing blocks after it (the lanes that were
 * still busy). Blocks beyond that were never started. An upper bound for the stopped run, not a count.
 */
export function resentBytes(
  need: number[],
  count: number,
  size: number,
  blockSize: number,
  lanes: number,
) {
  const missing = new Set(need);
  let lastHeld = -1;
  for (let i = count - 1; i >= 0 && lastHeld < 0; i -= 1) if (!missing.has(i)) lastHeld = i;
  const sorted = [...need].sort((a, b) => a - b);
  const maybeSent = [
    ...sorted.filter((i) => i < lastHeld),
    ...sorted.filter((i) => i > lastHeld).slice(0, lanes),
  ];
  return maybeSent.reduce((sum, i) => sum + Math.min(size, (i + 1) * blockSize) - i * blockSize, 0);
}
