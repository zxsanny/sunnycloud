import { afterEach, describe, expect, it, vi } from 'vitest';
import { parseVectors } from '../../../../sync-core-wasm/js/run-vectors.mjs';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { problem } from '../../../tests/support/http';
import { MIB, patternBytes, patternFile, sameBytes } from '../../../tests/support/files';
import { UploadServer } from '../../../tests/support/uploadServer';
import { hex, wasm } from '../../../tests/support/wasm';
import { createApi } from '$lib/api';
import { UploadEngine } from './engine';
import type { FromWorker } from './protocol';
import { rejectWhenAborted } from '../../../tests/support/abortable';

const PARENT = '0190aaaa-0000-7000-8000-00000000f001';

const vectors = parseVectors(
  readFileSync(join(process.cwd(), '../sync-core-wasm/vectors/sync_core_vectors.txt'), 'utf8'),
) as { kind: string; name: string; length: number; fileHash: string; blocks: string[] }[];
const vector = (name: string) => {
  const found = vectors.find((c) => c.kind === 'file' && c.name === name);
  if (!found) throw new Error(`no vector ${name}`);
  return found;
};

function rig(server: UploadServer) {
  const messages: FromWorker[] = [];
  const engine = new UploadEngine({
    wasm,
    api: createApi({ fetch: (request) => server.fetch(request) }),
    post: (m) => messages.push(m),
    sleep: () => Promise.resolve(),
  });
  const last = <T extends FromWorker['type']>(type: T) =>
    messages.filter((m): m is Extract<FromWorker, { type: T }> => m.type === type).at(-1);
  return { engine, messages, last };
}

afterEach(() => vi.restoreAllMocks());

describe('the small path: staged PUT and batch commit', () => {
  it.each([
    { length: 0, vector: 'empty' },
    { length: 2 * MIB - 1, vector: 'two_mib_minus_1' },
    { length: 2 * MIB, vector: 'block_2mib' },
  ])(
    'stages $length bytes with the exact body and the BLAKE3 of the shared vectors',
    async ({ length, vector: name }) => {
      // Arrange
      const server = new UploadServer();
      const { engine, last } = rig(server);
      const bytes = patternBytes(length);

      // Act
      await engine.handle({ type: 'stage', job: 'j1', file: new Blob([bytes]) });

      // Assert
      const key = last('staged')?.stagedKey ?? '';
      const stored = server.staged.get(key);
      expect(sameBytes(stored?.body, bytes)).toBe(true);
      expect(stored?.hash).toBe(
        length === 0 ? hex(wasm.hash_block(new Uint8Array())) : vector(name).blocks[0],
      );
    },
  );

  it('reports a staging refusal with the server code', async () => {
    // Arrange
    const server = new UploadServer({ maxStagedPerAccount: 0 });
    const { engine, last } = rig(server);

    // Act
    await engine.handle({ type: 'stage', job: 'j1', file: patternFile(10) });

    // Assert
    expect(last('stageFailed')?.failure).toMatchObject({
      kind: 'api',
      error: { code: 'staged_cap_reached' },
    });
  });

  it('reports an unreadable file as a local failure and sends nothing', async () => {
    // Arrange
    const server = new UploadServer();
    const { engine, last } = rig(server);
    const gone = {
      size: 10,
      slice: () => ({
        arrayBuffer: () => Promise.reject(new DOMException('gone', 'NotFoundError')),
      }),
    } as unknown as Blob;

    // Act
    await engine.handle({ type: 'stage', job: 'j1', file: gone });

    // Assert
    expect(last('stageFailed')?.failure).toEqual({ kind: 'local', reason: 'FileUnreadable' });
    expect(server.requests).toEqual([]);
  });

  it('commits a batch with a commit ID of its own and returns it', async () => {
    // Arrange
    const server = new UploadServer();
    const { engine, last } = rig(server);
    await engine.handle({ type: 'stage', job: 'j1', file: patternFile(100, 'a.bin') });
    const stagedKey = last('staged')?.stagedKey ?? '';

    // Act
    await engine.handle({
      type: 'batch',
      batch: 'b1',
      entries: [{ stagedKey, parentId: PARENT, name: 'a.bin' }],
    });

    // Assert
    const done = last('batchDone');
    expect(done?.commitId).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab]/);
    expect(done?.entries).toMatchObject([{ status: 'committed' }]);
    expect(server.nodes).toMatchObject([{ name: 'a.bin', parentId: PARENT, size: 100 }]);
  });

  it('reports a lost batch answer with its commit ID, and a retry with that ID gets the stored answer', async () => {
    // Arrange
    const server = new UploadServer();
    const { engine, last } = rig(server);
    await engine.handle({ type: 'stage', job: 'j1', file: patternFile(100, 'a.bin') });
    const entries = [
      { stagedKey: last('staged')?.stagedKey ?? '', parentId: PARENT, name: 'a.bin' },
    ];
    let cut = true;
    server.faults.push((request, route) => {
      if (route !== 'POST /api/v1/commits:batch' || !cut) return undefined;
      cut = false;
      // The server handles it; the answer never arrives.
      void server.fetch(request.clone()).then(() => undefined);
      throw new TypeError('fetch failed');
    });

    // Act
    await engine.handle({ type: 'batch', batch: 'b1', entries });
    const failed = last('batchFailed');
    await engine.handle({ type: 'batch', batch: 'b1', commitId: failed?.commitId, entries });

    // Assert
    expect(failed?.failure).toMatchObject({ kind: 'api', error: { kind: 'network' } });
    expect(last('batchDone')?.commitId).toBe(failed?.commitId);
    expect(server.nodes).toHaveLength(1);
  });
});

describe('the large path: session, blocks on lanes, commit', () => {
  it('sends 2 MiB + 1 byte as two blocks with the hashes of the shared vectors and commits their blocklist', async () => {
    // Arrange
    const server = new UploadServer({ keepBodies: true });
    const { engine, last } = rig(server);
    const bytes = patternBytes(2 * MIB + 1);

    // Act
    await engine.handle({
      type: 'upload',
      job: 'j1',
      file: new Blob([bytes]),
      parentId: PARENT,
      name: 'big.bin',
      lanes: 4,
    });

    // Assert
    const expected = vector('block_2mib_plus_1');
    expect([...server.blockLog].sort((a, b) => a.index - b.index)).toEqual([
      { index: 0, hash: expected.blocks[0], length: 2 * MIB },
      { index: 1, hash: expected.blocks[1], length: 1 },
    ]);
    const done = last('done');
    expect(done?.nodeId).toBeTruthy();
    expect(server.nodes[0]?.contentHash).toBe(expected.fileHash);
    expect(sameBytes(server.contents.get(done?.nodeId ?? ''), bytes)).toBe(true);
  });

  it('uploads a 5-block file on at most 4 lanes, one block per request, and the server gets every byte', async () => {
    // Arrange
    const server = new UploadServer({ keepBodies: true, blockDelayMs: 5 });
    const { engine, last } = rig(server);
    const bytes = patternBytes(4 * 2 * MIB + MIB);

    // Act
    await engine.handle({
      type: 'upload',
      job: 'j1',
      file: new Blob([bytes]),
      parentId: PARENT,
      name: 'five.bin',
      lanes: 4,
    });

    // Assert
    const done = last('done');
    expect(server.maxConcurrentBlocks).toBe(4);
    expect(
      server.requests.filter((r) => r.startsWith('PUT') && r.includes('/blocks/')),
    ).toHaveLength(5);
    expect(sameBytes(server.contents.get(done?.nodeId ?? ''), bytes)).toBe(true);
  });

  it('never uses more lanes than the server allows', async () => {
    // Arrange
    const server = new UploadServer({ blockDelayMs: 5, maxLanes: 2 });
    const { engine, last } = rig(server);

    // Act
    await engine.handle({
      type: 'upload',
      job: 'j1',
      file: patternFile(5 * 2 * MIB),
      parentId: PARENT,
      name: 'f.bin',
      lanes: 2,
    });

    // Assert
    expect(last('done')).toBeTruthy();
    expect(server.maxConcurrentBlocks).toBe(2);
  });

  it('reports progress from hashing to sending and ends at the full size', async () => {
    // Arrange
    const server = new UploadServer();
    const { engine, messages } = rig(server);

    // Act
    await engine.handle({
      type: 'upload',
      job: 'j1',
      file: patternFile(3 * 2 * MIB),
      parentId: PARENT,
      name: 'f.bin',
      lanes: 4,
    });

    // Assert
    const progress = messages.filter((m) => m.type === 'progress');
    expect(progress[0]).toMatchObject({ phase: 'preparing' });
    expect(progress.some((m) => m.type === 'progress' && m.phase === 'sending')).toBe(true);
  });

  it('resends a block after a transient server error', async () => {
    // Arrange
    const server = new UploadServer();
    const { engine, last } = rig(server);
    let failed = false;
    server.faults.push((_request, route) => {
      if (route !== 'PUT /api/v1/uploads/:id/blocks/:n' || failed) return undefined;
      failed = true;
      return problem(503, 'overloaded', { retry_after: 1 });
    });

    // Act
    await engine.handle({
      type: 'upload',
      job: 'j1',
      file: patternFile(2 * MIB + 5),
      parentId: PARENT,
      name: 'f.bin',
      lanes: 1,
    });

    // Assert
    expect(last('done')).toBeTruthy();
    expect(server.requests.filter((r) => r.includes('/blocks/'))).toHaveLength(3);
  });

  it('follows the planner after a block conflict: reads the session again, then sends what is still needed', async () => {
    // Arrange
    const server = new UploadServer();
    const { engine, last } = rig(server);
    let conflict = true;
    server.faults.push((_request, route) => {
      if (route !== 'PUT /api/v1/uploads/:id/blocks/:n' || !conflict) return undefined;
      conflict = false;
      return problem(409, 'block_conflict');
    });

    // Act
    await engine.handle({
      type: 'upload',
      job: 'j1',
      file: patternFile(2 * MIB + 5),
      parentId: PARENT,
      name: 'f.bin',
      lanes: 1,
    });

    // Assert
    expect(last('done')).toBeTruthy();
    expect(server.requests).toContain('GET /api/v1/uploads/:id');
  });

  it('resends a block the server refused for its hash', async () => {
    // Arrange
    const server = new UploadServer();
    const { engine, last } = rig(server);
    let refused = false;
    server.faults.push((_request, route) => {
      if (route !== 'PUT /api/v1/uploads/:id/blocks/:n' || refused) return undefined;
      refused = true;
      return problem(422, 'block_hash_mismatch');
    });

    // Act
    await engine.handle({
      type: 'upload',
      job: 'j1',
      file: patternFile(2 * MIB + 5),
      parentId: PARENT,
      name: 'f.bin',
      lanes: 1,
    });

    // Assert
    expect(last('done')).toBeTruthy();
  });

  it('gives up with a typed failure when a block never gets through', async () => {
    // Arrange
    const server = new UploadServer();
    const { engine, last } = rig(server);
    server.faults.push((_request, route) =>
      route === 'PUT /api/v1/uploads/:id/blocks/:n' ? problem(503, 'overloaded') : undefined,
    );

    // Act
    await engine.handle({
      type: 'upload',
      job: 'j1',
      file: patternFile(2 * MIB + 5),
      parentId: PARENT,
      name: 'f.bin',
      lanes: 1,
    });

    // Assert
    expect(last('failed')?.failure).toEqual({ kind: 'local', reason: 'BlockRetriesExhausted' });
    expect(last('failed')?.clientKey).toBeTruthy();
  });

  it('answers a refused session with the server code and sends no block', async () => {
    // Arrange
    const server = new UploadServer();
    const { engine, last } = rig(server);
    server.faults.push((_request, route) =>
      route === 'POST /api/v1/uploads' ? problem(507, 'quota_exceeded') : undefined,
    );

    // Act
    await engine.handle({
      type: 'upload',
      job: 'j1',
      file: patternFile(2 * MIB + 5),
      parentId: PARENT,
      name: 'f.bin',
      lanes: 4,
    });

    // Assert
    expect(last('failed')?.failure).toMatchObject({
      kind: 'api',
      error: { code: 'quota_exceeded', status: 507 },
    });
    expect(server.requests.some((r) => r.includes('/blocks/'))).toBe(false);
  });

  it('ends with no block sent when the server already holds the content', async () => {
    // Arrange
    const server = new UploadServer();
    const { engine, last } = rig(server);
    const file = patternFile(2 * MIB + 5);
    await engine.handle({
      type: 'upload',
      job: 'j1',
      file,
      parentId: PARENT,
      name: 'f.bin',
      lanes: 4,
    });
    const blocksBefore = server.requests.filter((r) => r.includes('/blocks/')).length;

    // Act
    await engine.handle({
      type: 'upload',
      job: 'j2',
      file,
      parentId: PARENT,
      name: 'f.bin',
      lanes: 4,
    });

    // Assert
    expect(last('done')).toMatchObject({ job: 'j2', nodeId: null });
    expect(server.requests.filter((r) => r.includes('/blocks/')).length).toBe(blocksBefore);
  });

  it('starts a new session when the old one is gone and still finishes', async () => {
    // Arrange
    const server = new UploadServer();
    const { engine, last } = rig(server);
    let gone = true;
    server.faults.push((_request, route) => {
      if (route !== 'PUT /api/v1/uploads/:id/blocks/:n' || !gone) return undefined;
      gone = false;
      return problem(410, 'upload_session_gone');
    });

    // Act
    await engine.handle({
      type: 'upload',
      job: 'j1',
      file: patternFile(2 * MIB + 5),
      parentId: PARENT,
      name: 'f.bin',
      lanes: 1,
    });

    // Assert
    expect(last('done')).toBeTruthy();
  });

  it('cancels in the middle of a block: aborts the request, sends no further block and aborts the session', async () => {
    // Arrange
    const server = new UploadServer();
    const { engine, last } = rig(server);
    let release!: () => void;
    const started = new Promise<void>((resolve) => (release = resolve));
    server.faults.push((request, route) => {
      if (route !== 'PUT /api/v1/uploads/:id/blocks/:n') return undefined;
      release();
      return new Promise<Response>((_resolve, reject) => rejectWhenAborted(request.signal, reject));
    });
    const running = engine.handle({
      type: 'upload',
      job: 'j1',
      file: patternFile(3 * 2 * MIB),
      parentId: PARENT,
      name: 'f.bin',
      lanes: 2,
    });
    await started;
    const blocksBefore = server.requests.filter((r) => r.includes('/blocks/')).length;

    // Act
    await engine.handle({ type: 'cancel', job: 'j1' });
    await running;

    // Assert
    expect(last('cancelled')).toMatchObject({ job: 'j1' });
    expect(last('done')).toBeUndefined();
    expect(server.requests.filter((r) => r.includes('/blocks/')).length).toBe(blocksBefore);
    expect(server.deleted).toHaveLength(1);
  });

  it('reports an unreadable block as a local failure', async () => {
    // Arrange
    const server = new UploadServer();
    const { engine, last } = rig(server);
    const real = patternBytes(2 * MIB + 5);
    let reads = 0;
    const flaky = {
      size: real.length,
      slice: (a: number, b: number) => ({
        arrayBuffer: () =>
          ++reads > 3
            ? Promise.reject(new DOMException('gone', 'NotFoundError'))
            : Promise.resolve(real.slice(a, b).buffer),
      }),
    } as unknown as Blob;

    // Act
    await engine.handle({
      type: 'upload',
      job: 'j1',
      file: flaky,
      parentId: PARENT,
      name: 'f.bin',
      lanes: 1,
    });

    // Assert
    expect(last('failed')?.failure).toEqual({ kind: 'local', reason: 'FileUnreadable' });
  });
});
