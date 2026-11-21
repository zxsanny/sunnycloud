import { describe, expect, it } from 'vitest';
import { fillTestBytes } from '../../../sync-core-wasm/js/run-vectors.mjs';
import { createApi } from '$lib/api';
import { DownloadEngine, type Sink } from '$lib/transfer/download';
import type { FromWorker } from '$lib/transfer/protocol';
import { hex, wasm } from '../support/wasm';

const MIB = 1024 * 1024;
const SIZE_MIB = Number(process.env.DOWNLOAD_CHECK_MIB ?? 1024);
const ID = '0190aaaa-0000-7000-8000-00000000d001';

/** The file hash of the shared-vector pattern of `size` bytes, hashed one block at a time. */
function patternFileHash(size: number): string {
  const block = new Uint8Array(2 * MIB);
  const hashes: number[] = [];
  for (let at = 0; at < size; at += block.length) {
    const length = Math.min(block.length, size - at);
    fillTestBytes(block, at);
    hashes.push(...wasm.hash_block(block.subarray(0, length)));
  }
  return hex(wasm.rollup(Uint8Array.from(hashes)));
}

/** A server that makes the bytes of a range on demand, so the check needs no file and no copy of one. */
function patternServer(size: number) {
  return async (request: Request): Promise<Response> => {
    const url = new URL(request.url);
    if (url.pathname === `/api/v1/nodes/${ID}/content`) {
      const [, from, to] = /^bytes=(\d+)-(\d+)$/.exec(request.headers.get('Range') ?? '') ?? [];
      const start = Number(from);
      const end = Number(to);
      let at = start;
      const body = new ReadableStream<Uint8Array>({
        pull(controller) {
          if (at > end) return controller.close();
          const chunk = new Uint8Array(Math.min(256 * 1024, end + 1 - at));
          fillTestBytes(chunk, at);
          at += chunk.length;
          controller.enqueue(chunk);
        },
      });
      return new Response(body, {
        status: 206,
        headers: { 'Content-Range': `bytes ${start}-${end}/${size}` },
      });
    }
    throw new Error(`unplanned request: ${url.pathname}`);
  };
}

async function download(sizeMib: number, keep: boolean) {
  const size = sizeMib * MIB;
  if (globalThis.gc) globalThis.gc();
  const baseline = process.memoryUsage().rss;
  let peak = baseline;
  const hash = patternFileHash(size);
  const messages: FromWorker[] = [];
  const kept: Uint8Array[] = [];
  let written = 0;
  const sink: Sink = {
    write: async (block) => {
      written += block.length;
      // The engine never yields to timers (only promises), so the memory is read where the blocks arrive.
      peak = Math.max(peak, process.memoryUsage.rss());
      if (keep) kept.push(block.slice());
    },
    commit: async () => null,
    discard: async () => {},
  };
  const engine = new DownloadEngine({
    wasm,
    api: createApi({ fetch: patternServer(size), origin: 'http://localhost' }),
    post: (m) => messages.push(m),
    openSink: async () => sink,
  });
  const started = performance.now();
  try {
    await engine.handle({
      type: 'download',
      job: 'big',
      source: { kind: 'node', id: ID },
      size,
      hash,
      sink: { kind: 'opfs', name: 'dl-big' },
    });
  } finally {
    peak = Math.max(peak, process.memoryUsage.rss());
  }
  const seconds = (performance.now() - started) / 1000;
  const growth = (peak - baseline) / MIB;
  console.log(
    `[download check] ${sizeMib} MiB${keep ? ' (control: the sink keeps every block)' : ''} in ${seconds.toFixed(1)} s, peak RSS growth ${growth.toFixed(1)} MiB`,
  );
  return { messages, written, growth, size };
}

describe('a large download through the Worker code path, with a generated server', () => {
  it(`downloads ${SIZE_MIB} MiB, verifies it and keeps memory bounded`, async () => {
    // Act
    const { messages, written, growth, size } = await download(SIZE_MIB, false);

    // Assert
    expect(messages.at(-1)).toMatchObject({ type: 'downloadDone' });
    expect(written).toBe(size);
    expect(growth).toBeLessThan(160);
  });

  it('sees memory when a sink keeps every block (the measurement can fail)', async () => {
    // Act
    const { growth } = await download(512, true);

    // Assert
    expect(growth).toBeGreaterThan(400);
  });
});
