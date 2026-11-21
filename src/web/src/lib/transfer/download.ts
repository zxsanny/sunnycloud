import type { Api, ApiError } from '$lib/api';
import { hex } from './blocks';
import type { DownloadFailure, DownloadSink, FromWorker, ToWorker } from './protocol';
import type { WasmApi } from './wasm';

/** A place for verified bytes. Nothing is final until `commit`; `discard` leaves nothing the person could take for good. */
export interface Sink {
  write(block: Uint8Array): Promise<void>;
  /** The bytes are good. Returns the finished file for a private (OPFS) sink, which the page hands to the browser. */
  commit(): Promise<File | null>;
  discard(): Promise<void>;
}

export interface DownloadDeps {
  wasm: WasmApi;
  api: Api;
  post: (message: FromWorker) => void;
  openSink: (spec: DownloadSink) => Promise<Sink>;
}

type DownloadMessage = Extract<ToWorker, { type: 'download' }>;

/** One ranged request reads four blocks: big enough to keep the link busy, small enough to bound the heap. */
const BLOCKS_PER_RANGE = 4;
const PROGRESS_EVERY_MS = 100;

class Ended extends Error {
  constructor(readonly reason: DownloadFailure) {
    super(reason);
  }
}

export function downloadFailureOf(error: ApiError): DownloadFailure {
  if (error.kind === 'http') {
    if (error.status === 403 || error.status === 401) return 'Forbidden';
    if (error.status === 404) return 'NotFound';
  }
  return 'Network';
}

/**
 * The Worker's download logic, apart from the Worker shell so Node can run it. The file is read in ranged pulls of
 * four 2 MiB blocks, each block is hashed with sync-core-wasm and written as it arrives, and the roll-up of the block
 * hashes must equal the server's file hash before the sink commits. A mismatch discards everything.
 */
export class DownloadEngine {
  constructor(private readonly deps: DownloadDeps) {}

  /** Never rejects: every end is reported to the page as `downloadDone` or `downloadFailed`. */
  async handle(m: DownloadMessage): Promise<void> {
    const { post } = this.deps;
    let sink: Sink;
    try {
      sink = await this.deps.openSink(m.sink);
    } catch (cause) {
      console.error('The place for the download could not be opened.', cause);
      post({ type: 'downloadFailed', job: m.job, reason: 'SaveFailed' });
      return;
    }
    try {
      const file = await this.transfer(m, sink);
      post({ type: 'downloadDone', job: m.job, file: file ?? undefined });
    } catch (cause) {
      await this.discard(sink);
      if (cause instanceof Ended) {
        post({ type: 'downloadFailed', job: m.job, reason: cause.reason });
      } else {
        console.error('The download engine hit an unexpected error.', cause);
        post({ type: 'downloadFailed', job: m.job, reason: 'Network' });
      }
    }
  }

  private async discard(sink: Sink) {
    try {
      await sink.discard();
    } catch (cause) {
      // The browser may refuse to remove the file; the mismatch or failure is still what the page reports.
      console.warn('The partial download could not be removed.', cause);
    }
  }

  private async transfer(m: DownloadMessage, sink: Sink): Promise<File | null> {
    const { wasm } = this.deps;
    const block = wasm.block_size();
    const count = Math.ceil(m.size / block);
    const hashes = new Uint8Array(count * 32);
    const buffer = new Uint8Array(block);
    let index = 0;
    let done = 0;
    let lastPost = 0;

    const finishBlock = async (length: number) => {
      const bytes = buffer.subarray(0, length);
      hashes.set(wasm.hash_block(bytes), index * 32);
      index += 1;
      try {
        await sink.write(bytes);
      } catch (cause) {
        console.error('A block could not be written.', cause);
        throw new Ended('SaveFailed');
      }
      done += length;
      const now = Date.now();
      if (now - lastPost >= PROGRESS_EVERY_MS) {
        lastPost = now;
        this.deps.post({ type: 'downloadProgress', job: m.job, bytesDone: done });
      }
    };

    for (let start = 0; start < m.size; start += block * BLOCKS_PER_RANGE) {
      const end = Math.min(m.size, start + block * BLOCKS_PER_RANGE) - 1;
      const answer = await this.deps.api.getContentRange(m.source, start, end);
      if (!answer.ok) throw new Ended(downloadFailureOf(answer.error));
      // 200 is the whole file, which only a request for all of it may get; anything else is not the bytes asked for.
      const { status, body } = answer.value;
      const whole = start === 0 && end + 1 === m.size;
      if (body === null || !(status === 206 || (status === 200 && whole)))
        throw new Ended('Network');
      await this.readRange(body, end - start + 1, buffer, finishBlock);
    }
    if (index !== count) throw new Ended('Network');
    if (hex(wasm.rollup(hashes)) !== m.hash.toLowerCase()) throw new Ended('HashMismatch');
    this.deps.post({ type: 'downloadProgress', job: m.job, bytesDone: m.size });
    try {
      return await sink.commit();
    } catch (cause) {
      console.error('The file could not be finished.', cause);
      throw new Ended('SaveFailed');
    }
  }

  /** Fills `buffer` from the stream; every full buffer (and the last, shorter one) goes to `finishBlock`. */
  private async readRange(
    body: ReadableStream<Uint8Array>,
    expected: number,
    buffer: Uint8Array,
    finishBlock: (length: number) => Promise<void>,
  ) {
    const reader = body.getReader();
    let filled = 0;
    let total = 0;
    try {
      for (;;) {
        let chunk: ReadableStreamReadResult<Uint8Array>;
        try {
          chunk = await reader.read();
        } catch (cause) {
          console.warn('The download stream ended with an error.', cause);
          throw new Ended('Network');
        }
        if (chunk.done) break;
        let from = 0;
        while (from < chunk.value.length) {
          const take = Math.min(chunk.value.length - from, buffer.length - filled);
          buffer.set(chunk.value.subarray(from, from + take), filled);
          filled += take;
          from += take;
          total += take;
          if (filled === buffer.length) {
            await finishBlock(filled);
            filled = 0;
          }
        }
      }
    } finally {
      reader.releaseLock();
    }
    if (total !== expected) throw new Ended('Network');
    if (filled > 0) await finishBlock(filled);
  }
}
