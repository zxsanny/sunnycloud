import { createWriteStream } from 'node:fs';
import { openAsBlob } from 'node:fs';
import { open } from 'node:fs/promises';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { once } from 'node:events';
import { describe, expect, it } from 'vitest';
import { fillTestBytes } from '../../../sync-core-wasm/js/run-vectors.mjs';
import { createApi } from '$lib/api';
import { UploadEngine } from '$lib/transfer/engine';
import type { FromWorker } from '$lib/transfer/protocol';
import { UploadServer } from '../support/uploadServer';
import { wasm } from '../support/wasm';

const MIB = 1024 * 1024;
const SIZE_MIB = Number(process.env.LARGE_CHECK_MIB ?? 1024);
const PARENT = '0190aaaa-0000-7000-8000-00000000f001';

/** Writes a file of the shared-vector pattern (byte i is i mod 251) to disk, 8 MiB at a time. */
async function writePatternFile(path: string, mib: number) {
  const out = createWriteStream(path);
  const chunk = new Uint8Array(8 * MIB);
  for (let offset = 0; offset < mib * MIB; offset += chunk.length) {
    const length = Math.min(chunk.length, mib * MIB - offset);
    fillTestBytes(chunk, offset);
    if (!out.write(chunk.subarray(0, length))) await once(out, 'drain');
  }
  out.end();
  await once(out, 'finish');
}

/**
 * A file on disk read slice by slice, like a browser `File`. Node's `openAsBlob` reports size 0 from 4 GiB up, so
 * larger files use this.
 */
async function diskBlob(path: string, size: number): Promise<Blob> {
  const handle = await open(path, 'r');
  return {
    size,
    slice: (start: number, end: number) => ({
      arrayBuffer: async () => {
        const buffer = new Uint8Array(end - start);
        await handle.read(buffer, 0, buffer.length, start);
        return buffer.buffer;
      },
    }),
  } as unknown as Blob;
}

describe('a large file through the Worker code path, with the fake server', () => {
  it(`uploads ${SIZE_MIB} MiB and keeps memory bounded`, async () => {
    // Arrange
    const dir = await mkdtemp(join(process.env.LARGE_CHECK_DIR ?? tmpdir(), 'sunnycloud-large-'));
    const path = join(dir, 'large.bin');
    await writePatternFile(path, SIZE_MIB);
    const file = SIZE_MIB >= 4096 ? await diskBlob(path, SIZE_MIB * MIB) : await openAsBlob(path);
    const server = new UploadServer({ blockDelayMs: 2 });
    const messages: FromWorker[] = [];
    const engine = new UploadEngine({
      wasm,
      api: createApi({ fetch: (request) => server.fetch(request) }),
      post: (m) => messages.push(m),
    });
    if (globalThis.gc) globalThis.gc();
    const baseline = process.memoryUsage().rss;
    let peak = baseline;
    const sampler = setInterval(() => {
      peak = Math.max(peak, process.memoryUsage().rss);
    }, 50);
    const started = performance.now();

    // Act
    try {
      await engine.handle({
        type: 'upload',
        job: 'large',
        file,
        parentId: PARENT,
        name: 'large.bin',
        lanes: 4,
      });
    } finally {
      clearInterval(sampler);
      peak = Math.max(peak, process.memoryUsage().rss);
      await rm(dir, { recursive: true, force: true });
    }

    // Assert
    const seconds = (performance.now() - started) / 1000;
    const mb = (bytes: number) => (bytes / MIB).toFixed(0);
    console.log(
      `LARGE-FILE-RESULT size=${SIZE_MIB} MiB blocks=${SIZE_MIB / 2} baseline_rss=${mb(baseline)} MiB peak_rss=${mb(peak)} MiB ` +
        `growth=${mb(peak - baseline)} MiB time=${seconds.toFixed(1)} s throughput=${(SIZE_MIB / seconds).toFixed(0)} MiB/s ` +
        `max_concurrent_blocks=${server.maxConcurrentBlocks}`,
    );
    expect(messages.at(-1)).toMatchObject({ type: 'done' });
    expect(server.nodes[0]?.size).toBe(SIZE_MIB * MIB);
    expect(server.maxConcurrentBlocks).toBeLessThanOrEqual(4);
    // A whole-file buffer would add the file size; four lanes of 2 MiB blocks stay far below it.
    expect(peak - baseline).toBeLessThan(400 * MIB);
  });
});
