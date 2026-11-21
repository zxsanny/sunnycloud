import { describe, expect, it } from 'vitest';
import { MIB, patternBytes, patternFile, sameBytes } from '../../../tests/support/files';
import { UploadServer } from '../../../tests/support/uploadServer';
import { hex, wasm } from '../../../tests/support/wasm';
import { createApi } from '$lib/api';
import { resentBytes } from './blocks';
import { UploadEngine } from './engine';
import type { FromWorker, ToWorker } from './protocol';
import type { ResumeRecord } from './resume';

const PARENT = '0190aaaa-0000-7000-8000-00000000f001';
const KEY = '0190bbbb-0000-7000-8000-000000000001';
/** Six blocks: five of 2 MiB and a short one. */
const SIZE = 10 * MIB + 123;
const BLOCKS = 6;

type Resume = Extract<ToWorker, { type: 'resume' }>;

function rig(server: UploadServer) {
  const messages: FromWorker[] = [];
  const hashed = { blocks: 0 };
  const counting = {
    block_size: wasm.block_size,
    hash_block: (block: Uint8Array | number[]) => {
      hashed.blocks += 1;
      return wasm.hash_block(block);
    },
    rollup: wasm.rollup,
    new_commit_id: wasm.new_commit_id,
    UploadPlanner: wasm.UploadPlanner,
  };
  const engine = new UploadEngine({
    wasm: counting,
    api: createApi({ fetch: (request) => server.fetch(request) }),
    post: (m) => messages.push(m),
    sleep: () => Promise.resolve(),
  });
  const last = <T extends FromWorker['type']>(type: T) =>
    messages.filter((m): m is Extract<FromWorker, { type: T }> => m.type === type).at(-1);
  return { engine, messages, last, hashed };
}

/** What a closed tab leaves: a session on the server that holds the `held` blocks, and the record of it. */
async function stoppedUpload(server: UploadServer, file: File, held: number[]) {
  const api = createApi({ fetch: (request) => server.fetch(request) });
  const bytes = patternBytes(file.size);
  const hashes = Array.from({ length: BLOCKS }, (_, i) =>
    wasm.hash_block(bytes.subarray(i * 2 * MIB, Math.min(file.size, (i + 1) * 2 * MIB))),
  );
  const started = await api.postUploads({
    parent_id: PARENT,
    name: file.name,
    declared_size: file.size,
    client_key: KEY,
    content_hash: hex(wasm.rollup(Uint8Array.from(hashes.flatMap((h) => [...h])))),
  });
  if (!started.ok || !started.value.upload_id) throw new Error('the session did not start');
  const sessionId = started.value.upload_id;
  for (const i of held) {
    const body = bytes.slice(i * 2 * MIB, Math.min(file.size, (i + 1) * 2 * MIB));
    await api.putBlock(sessionId, i, body, hex(hashes[i] as Uint8Array));
  }
  const record: ResumeRecord = {
    fileName: file.name,
    size: file.size,
    lastModified: file.lastModified,
    sessionId,
    clientKey: KEY,
    blocklist: hashes.map(hex),
    parentId: PARENT,
  };
  return record;
}

const resumeMessage = (record: ResumeRecord, file: Blob, over: Partial<Resume> = {}): Resume => ({
  type: 'resume',
  job: 'j1',
  file,
  parentId: record.parentId,
  name: record.fileName,
  lanes: 4,
  clientKey: record.clientKey,
  sessionId: record.sessionId,
  blocklist: record.blocklist,
  clientMtime: record.lastModified,
  ...over,
});

const blockPuts = (server: UploadServer) =>
  server.requests.filter((r) => r === 'PUT /api/v1/uploads/:id/blocks/:n').length;

describe('resuming a stopped upload in the engine', () => {
  it('sends only the blocks the server lacks, with no hash pass and the same client key', async () => {
    // Arrange
    const server = new UploadServer({ keepBodies: true });
    const { engine, last, hashed } = rig(server);
    const file = patternFile(SIZE, 'big.bin');
    const record = await stoppedUpload(server, file, [0, 1, 3]);
    server.requests.length = 0;

    // Act
    await engine.handle(resumeMessage(record, file));

    // Assert
    expect(last('done')?.clientKey).toBe(KEY);
    expect(hashed.blocks).toBe(0);
    expect(blockPuts(server)).toBe(3);
    expect(server.requests).not.toContain('POST /api/v1/uploads');
    expect(server.blockLog.map((b) => b.index).sort()).toEqual([0, 1, 2, 3, 4, 5]);
    expect(sameBytes(server.contents.get(server.nodes[0]?.nodeId ?? ''), patternBytes(SIZE))).toBe(
      true,
    );
  });

  it('answers with the bytes it may send again: the holes before the last held block and the busy lanes after it', async () => {
    // Arrange
    const server = new UploadServer();
    const { engine, last } = rig(server);
    const file = patternFile(SIZE, 'big.bin');
    const record = await stoppedUpload(server, file, [0, 1, 3]);

    // Act
    await engine.handle(resumeMessage(record, file));

    // Assert: block 2 is a hole; blocks 4 and 5 (the last one short) are within the four lanes after block 3.
    expect(last('resumed')?.resentBytes).toBe(4 * MIB + 123);
  });

  it('computes what may be sent again from the blocks still needed', () => {
    // Arrange: three blocks of 2, 2 and 1 MiB.
    const size = 5 * MIB;
    const count = 3;
    const bytes = (need: number[], lanes = 4) => resentBytes(need, count, size, 2 * MIB, lanes);

    // Act and Assert
    expect(bytes([0, 2])).toBe(3 * MIB);
    expect(bytes([1, 2])).toBe(3 * MIB);
    expect(bytes([0, 1, 2], 2)).toBe(4 * MIB);
    expect(bytes([])).toBe(0);
  });

  it.each([
    { what: 'a block the server still needs', index: 4 },
    { what: 'a block the server already holds', index: 1 },
  ])(
    'restarts with a fresh hash pass when the record has a wrong hash for $what, and commits the real content',
    async ({ index }) => {
      // Arrange
      const server = new UploadServer({ keepBodies: true });
      const { engine, last, hashed } = rig(server);
      const file = patternFile(SIZE, 'big.bin');
      const record = await stoppedUpload(server, file, [0, 1, 3]);
      const blocklist = [...record.blocklist];
      blocklist[index] = `${'0'.repeat(63)}1`;

      // Act
      await engine.handle(resumeMessage(record, file, { blocklist }));

      // Assert
      expect(last('done')).toBeDefined();
      expect(hashed.blocks).toBeGreaterThanOrEqual(BLOCKS);
      expect(server.nodes).toHaveLength(1);
      expect(
        sameBytes(server.contents.get(server.nodes[0]?.nodeId ?? ''), patternBytes(SIZE)),
      ).toBe(true);
    },
  );

  it('hashes the file again when the record has the wrong number of hashes', async () => {
    // Arrange
    const server = new UploadServer({ keepBodies: true });
    const { engine, last, hashed } = rig(server);
    const file = patternFile(SIZE, 'big.bin');
    const record = await stoppedUpload(server, file, [0, 1]);

    // Act
    await engine.handle(resumeMessage(record, file, { blocklist: record.blocklist.slice(1) }));

    // Assert
    expect(last('done')).toBeDefined();
    expect(last('resumed')).toBeDefined();
    expect(hashed.blocks).toBe(BLOCKS);
    expect(sameBytes(server.contents.get(server.nodes[0]?.nodeId ?? ''), patternBytes(SIZE))).toBe(
      true,
    );
  });

  it('stops with SessionGone when the server no longer has the session, and sends no block', async () => {
    // Arrange
    const server = new UploadServer();
    const { engine, last } = rig(server);
    const file = patternFile(SIZE, 'big.bin');
    const record = await stoppedUpload(server, file, [0, 1]);
    server.sessions.clear();
    server.requests.length = 0;

    // Act
    await engine.handle(resumeMessage(record, file));

    // Assert
    expect(last('failed')?.failure).toEqual({ kind: 'local', reason: 'SessionGone' });
    expect(blockPuts(server)).toBe(0);
  });

  it('stops with FileUnreadable before asking the server anything when the file cannot be read', async () => {
    // Arrange
    const server = new UploadServer();
    const { engine, last } = rig(server);
    const file = patternFile(SIZE, 'big.bin');
    const record = await stoppedUpload(server, file, [0]);
    server.requests.length = 0;
    const unreadable = {
      size: SIZE,
      slice: () => ({
        arrayBuffer: () => Promise.reject(new DOMException('gone', 'NotFoundError')),
      }),
    } as unknown as Blob;

    // Act
    await engine.handle(resumeMessage(record, unreadable));

    // Assert
    expect(last('failed')?.failure).toEqual({ kind: 'local', reason: 'FileUnreadable' });
    expect(server.requests).toEqual([]);
  });

  it('tells the page about every new session so that it can keep a record', async () => {
    // Arrange
    const server = new UploadServer();
    const { engine, last } = rig(server);

    // Act
    await engine.handle({
      type: 'upload',
      job: 'j1',
      file: patternFile(SIZE, 'big.bin'),
      parentId: PARENT,
      name: 'big.bin',
      lanes: 4,
    });

    // Assert
    const session = last('session');
    expect(session?.blocklist).toHaveLength(BLOCKS);
    expect(session?.clientKey).toBe(last('done')?.clientKey);
    expect(session?.sessionId).toMatch(/^[0-9a-f]{8}-[0-9a-f-]{27}$/);
  });
});

/** A file that looks the same until `reads` slices were taken, then differs the way the test says. */
function editedFile(base: File, edit: { afterReads: number; how: 'unreadable' | 'newTime' }) {
  let reads = 0;
  return {
    size: base.size,
    get lastModified() {
      return edit.how === 'newTime' && reads >= edit.afterReads
        ? base.lastModified + 1000
        : base.lastModified;
    },
    slice(start: number, end: number) {
      reads += 1;
      if (edit.how === 'unreadable' && reads > edit.afterReads) {
        return {
          arrayBuffer: () => Promise.reject(new DOMException('changed', 'NotReadableError')),
        };
      }
      return base.slice(start, end);
    },
  } as unknown as Blob;
}

describe('a file edited before commit', () => {
  it.each([
    { how: 'newTime' as const, what: 'its modified time changes' },
    { how: 'unreadable' as const, what: 'the browser refuses to read it any more' },
  ])('stops with FileChanged and commits nothing when $what', async ({ how }) => {
    // Arrange
    const server = new UploadServer();
    const { engine, last } = rig(server);
    // One read per block for the hash pass and one per block to send; the next read is the check before commit.
    const file = editedFile(patternFile(SIZE, 'big.bin'), { afterReads: 2 * BLOCKS, how });

    // Act
    await engine.handle({
      type: 'upload',
      job: 'j1',
      file,
      parentId: PARENT,
      name: 'big.bin',
      lanes: 4,
    });

    // Assert
    expect(last('failed')?.failure).toEqual({ kind: 'local', reason: 'FileChanged' });
    expect(server.requests).not.toContain('POST /api/v1/commits');
    expect(server.nodes).toEqual([]);
  });

  it('commits a file that did not change', async () => {
    // Arrange
    const server = new UploadServer();
    const { engine, last } = rig(server);

    // Act
    await engine.handle({
      type: 'upload',
      job: 'j1',
      file: patternFile(SIZE, 'big.bin'),
      parentId: PARENT,
      name: 'big.bin',
      lanes: 4,
    });

    // Assert
    expect(last('done')).toBeDefined();
    expect(server.nodes).toHaveLength(1);
  });
});
