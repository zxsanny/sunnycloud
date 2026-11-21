import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { parseVectors } from '../../../../sync-core-wasm/js/run-vectors.mjs';
import { ContentServer, fileHashOf } from '../../../tests/support/contentServer';
import { MIB, sameBytes } from '../../../tests/support/files';
import { MemorySink } from '../../../tests/support/memorySink';
import { wasm } from '../../../tests/support/wasm';
import { createApi } from '$lib/api';
import { DownloadEngine } from './download';
import type { FromWorker } from './protocol';

const vectors = parseVectors(
  readFileSync(join(process.cwd(), '../sync-core-wasm/vectors/sync_core_vectors.txt'), 'utf8'),
) as { kind: string; name: string; length: number; fileHash: string }[];
const vector = (name: string) => {
  const found = vectors.find((c) => c.kind === 'file' && c.name === name);
  if (!found) throw new Error(`no vector ${name}`);
  return found;
};

function rig(server: ContentServer) {
  const messages: FromWorker[] = [];
  const sink = new MemorySink();
  const hashed: number[] = [];
  const counting = {
    ...wasm,
    hash_block: (block: Uint8Array | number[]) => {
      hashed.push(block.length);
      return wasm.hash_block(block);
    },
  };
  const engine = new DownloadEngine({
    wasm: counting,
    api: createApi({ fetch: (request) => server.fetch(request) }),
    post: (m) => messages.push(m),
    openSink: () => Promise.resolve(sink),
  });
  const last = () =>
    messages.filter((m) => m.type.startsWith('downloadDone') || m.type === 'downloadFailed').at(-1);
  const run = (id: string, size: number, hash: string, kind: 'node' | 'revision' = 'node') =>
    engine.handle({
      type: 'download',
      job: 'j1',
      source: { kind, id },
      size,
      hash,
      sink: { kind: 'opfs', name: 'dl-j1' },
    });
  return { run, sink, messages, hashed, last };
}

afterEach(() => vi.restoreAllMocks());

describe('a verified download: per-block hashing and the roll-up against the server hash', () => {
  it.each([
    { name: 'two_mib_minus_1', length: 2 * MIB - 1 },
    { name: 'block_2mib', length: 2 * MIB },
    { name: 'six_mib', length: 6 * MIB },
  ])(
    'saves $length bytes whose roll-up equals the hash of the shared vectors',
    async ({ name, length }) => {
      // Arrange
      const server = new ContentServer();
      const file = server.add('n1', length);
      const { run, sink, last, hashed } = rig(server);
      const expected = vector(name).fileHash;

      // Act
      await run('n1', length, expected);

      // Assert
      expect(last()).toMatchObject({ type: 'downloadDone' });
      expect(sink.committed).toBe(true);
      expect(sink.discarded).toBe(false);
      expect(sameBytes(sink.bytes, file.bytes)).toBe(true);
      expect(fileHashOf(sink.bytes)).toBe(expected);
      expect(hashed.every((n) => n <= 2 * MIB)).toBe(true);
    },
  );

  it('hashes and writes one 2 MiB block at a time, reading the file in ranges of four blocks', async () => {
    // Arrange: 21 MiB is 11 blocks (10 whole and a 1 MiB tail) and three ranges.
    const server = new ContentServer();
    const bytes = server.add('n1', 21 * MIB).bytes;
    const { run, sink, hashed } = rig(server);

    // Act
    await run('n1', bytes.length, fileHashOf(bytes));

    // Assert
    expect(hashed).toEqual([...Array<number>(10).fill(2 * MIB), MIB]);
    expect(sink.blocks.map((b) => b.length)).toEqual(hashed);
    expect(server.requests.filter((r) => r.range !== null).map((r) => r.range)).toEqual([
      `bytes=0-${8 * MIB - 1}`,
      `bytes=${8 * MIB}-${16 * MIB - 1}`,
      `bytes=${16 * MIB}-${21 * MIB - 1}`,
    ]);
  });

  it('commits an empty file without asking the server for any byte', async () => {
    // Arrange
    const server = new ContentServer();
    server.add('n1', 0);
    const { run, sink, last } = rig(server);

    // Act
    await run('n1', 0, vector('empty').fileHash);

    // Assert
    expect(last()).toMatchObject({ type: 'downloadDone' });
    expect(sink.committed).toBe(true);
    expect(server.requests).toEqual([]);
  });

  it('reads a named revision through its own content route', async () => {
    // Arrange
    const server = new ContentServer();
    const bytes = server.add('n1', 3 * MIB).bytes;
    const { run, last } = rig(server);

    // Act
    await run('rev-n1', bytes.length, fileHashOf(bytes), 'revision');

    // Assert
    expect(last()).toMatchObject({ type: 'downloadDone' });
    expect(server.requests[0]?.route).toBe('GET /api/v1/revisions/rev-n1/content');
  });
});

describe('a download that is not the file the server holds is never kept as good', () => {
  it.each([
    { where: 'the first block', at: 17 },
    { where: 'a middle block', at: 2 * MIB + 5 },
    { where: 'the last byte', at: 5 * MIB - 1 },
  ])('flipping one bit in $where ends as HashMismatch and discards the file', async ({ at }) => {
    // Arrange
    const server = new ContentServer();
    const bytes = server.add('n1', 5 * MIB).bytes;
    const expected = fileHashOf(bytes);
    server.fault = { flipAt: at };
    const { run, sink, last, messages } = rig(server);

    // Act
    await run('n1', bytes.length, expected);

    // Assert
    expect(last()).toEqual({ type: 'downloadFailed', job: 'j1', reason: 'HashMismatch' });
    expect(sink.committed).toBe(false);
    expect(sink.discarded).toBe(true);
    expect(messages.some((m) => m.type === 'downloadDone')).toBe(false);
  });

  it('treats a body that ends early as an unfinished download, not as a short good file', async () => {
    // Arrange
    const server = new ContentServer();
    const bytes = server.add('n1', 5 * MIB).bytes;
    server.fault = { cutAfter: MIB };
    const { run, sink, last } = rig(server);
    vi.spyOn(console, 'warn').mockImplementation(() => {});

    // Act
    await run('n1', bytes.length, fileHashOf(bytes));

    // Assert
    expect(last()).toMatchObject({ type: 'downloadFailed', reason: 'Network' });
    expect(sink.committed).toBe(false);
    expect(sink.discarded).toBe(true);
  });

  it('refuses a whole-file answer to a request for the middle of the file', async () => {
    // Arrange
    const server = new ContentServer();
    const bytes = server.add('n1', 20 * MIB).bytes;
    server.fault = { ignoreRange: true };
    const { run, sink, last } = rig(server);

    // Act
    await run('n1', bytes.length, fileHashOf(bytes));

    // Assert: the first range is 8 MiB of a 20 MiB file, so a 200 is not what was asked for.
    expect(last()).toMatchObject({ type: 'downloadFailed', reason: 'Network' });
    expect(sink.discarded).toBe(true);
  });

  it('does not trust the status: right bytes with a wrong hash from the metadata still fail', async () => {
    // Arrange
    const server = new ContentServer();
    const bytes = server.add('n1', 3 * MIB).bytes;
    const { run, sink, last } = rig(server);

    // Act
    await run('n1', bytes.length, '0'.repeat(64));

    // Assert
    expect(last()).toMatchObject({ type: 'downloadFailed', reason: 'HashMismatch' });
    expect(sink.committed).toBe(false);
  });

  it.each([
    { id: 'gone', reason: 'NotFound' },
    { id: 'rev-n1', reason: 'NotFound' },
  ])('maps the server answer for $id to $reason and keeps nothing', async ({ id, reason }) => {
    // Arrange
    const server = new ContentServer();
    server.add('n1', 3 * MIB);
    server.retired.add('rev-n1');
    const { run, sink, last } = rig(server);

    // Act
    await run(id, 3 * MIB, '0'.repeat(64), id.startsWith('rev') ? 'revision' : 'node');

    // Assert
    expect(last()).toMatchObject({ type: 'downloadFailed', reason });
    expect(sink.committed).toBe(false);
    expect(sink.discarded).toBe(true);
  });

  it('reports a write error as SaveFailed and discards', async () => {
    // Arrange
    const server = new ContentServer();
    const bytes = server.add('n1', 3 * MIB).bytes;
    const { run, sink, last } = rig(server);
    sink.failWrites = true;
    vi.spyOn(console, 'error').mockImplementation(() => {});

    // Act
    await run('n1', bytes.length, fileHashOf(bytes));

    // Assert
    expect(last()).toMatchObject({ type: 'downloadFailed', reason: 'SaveFailed' });
    expect(sink.discarded).toBe(true);
  });

  it('still reports the mismatch when the partial file cannot be removed', async () => {
    // Arrange
    const server = new ContentServer();
    const bytes = server.add('n1', 3 * MIB).bytes;
    server.fault = { flipAt: 1 };
    const { run, sink, last } = rig(server);
    sink.discard = () => Promise.reject(new DOMException('locked', 'NoModificationAllowedError'));
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});

    // Act
    await run('n1', bytes.length, fileHashOf(bytes));

    // Assert
    expect(last()).toMatchObject({ type: 'downloadFailed', reason: 'HashMismatch' });
    expect(warn).toHaveBeenCalledOnce();
  });
});
