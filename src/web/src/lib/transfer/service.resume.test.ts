import { IDBFactory, IDBKeyRange } from 'fake-indexeddb';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MIB, patternBytes, patternFile, sameBytes } from '../../../tests/support/files';
import { inProcessWorker, pageApi } from '../../../tests/support/inProcessWorker';
import { UploadServer } from '../../../tests/support/uploadServer';
import { STOPPED_TEXT } from './errors';
import { ResumeStore, type ResumeRecord } from './resume';
import { TransferService, type JobProgress } from '.';
import { rejectWhenAborted } from '../../../tests/support/abortable';

const PARENT = '0190aaaa-0000-7000-8000-00000000f001';
/** Six blocks: five of 2 MiB and a short one. */
const SIZE = 10 * MIB + 123;
const BLOCK_PUT = 'PUT /api/v1/uploads/:id/blocks/:n';

beforeEach(() => vi.stubGlobal('IDBKeyRange', IDBKeyRange));
afterEach(() => vi.unstubAllGlobals());

/** The browser (one IndexedDB) and the server; a tab is a `TransferService` with its own Worker. */
function world(account = 'account-a') {
  const server = new UploadServer({ keepBodies: true });
  const idb = new IDBFactory();
  const onUnavailable = vi.fn();
  const records = new ResumeStore(onUnavailable, () => idb);
  const tab = (accountId: string | null = account, store = records) => {
    const worker = inProcessWorker(server);
    return new TransferService({
      api: pageApi(server),
      connect: worker.connect,
      batchRetryMs: 0,
      resume: store,
      accountId: () => accountId,
    });
  };
  return { server, records, tab, idb, onUnavailable };
}

const blockIndex = (request: Request) => Number(new URL(request.url).pathname.split('/').pop());
const done = (jobs: JobProgress[]) =>
  jobs.every((j) => ['done', 'failed', 'cancelled', 'stopped'].includes(j.state));
const settle = (service: TransferService) =>
  vi.waitFor(() => expect(done(service.jobs)).toBe(true), { timeout: 15_000 });

/** Block PUTs from `from` on never answer, as when the tab closes with those blocks on the wire. */
function stallBlocksFrom(server: UploadServer, from: number, attempts: number[]) {
  server.faults.push((request, route) => {
    if (route !== BLOCK_PUT) return undefined;
    attempts.push(blockIndex(request));
    return blockIndex(request) >= from ? new Promise<Response>(() => undefined) : undefined;
  });
}

async function stopTab(
  w: ReturnType<typeof world>,
  file: File,
  stallFrom = 3,
  attempts: number[] = [],
) {
  stallBlocksFrom(w.server, stallFrom, attempts);
  const first = w.tab();
  await first.enqueueUpload([file], PARENT);
  await vi.waitFor(() => expect(w.server.blockLog.length).toBe(stallFrom), { timeout: 15_000 });
  await vi.waitFor(async () => expect(await w.records.list('account-a')).toHaveLength(1));
  first.suspend();
  w.server.faults.length = 0;
  return first;
}

describe('the resume record while an upload runs', () => {
  it('holds the contract fields and nothing else while the upload runs, and is gone after the commit', async () => {
    // Arrange
    const w = world();
    let release = () => {};
    const gate = new Promise<void>((resolve) => (release = resolve));
    w.server.faults.push((request, route) =>
      route === BLOCK_PUT ? gate.then(() => w.server.bypass(request)) : undefined,
    );
    const file = patternFile(SIZE, 'big.bin');
    const service = w.tab();

    // Act
    await service.enqueueUpload([file], PARENT);
    await vi.waitFor(async () => expect(await w.records.list('account-a')).toHaveLength(1));
    const [record] = await w.records.list('account-a');

    // Assert
    expect(Object.keys(record as ResumeRecord).sort()).toEqual(
      [
        'blocklist',
        'clientKey',
        'fileName',
        'lastModified',
        'parentId',
        'sessionId',
        'size',
      ].sort(),
    );
    expect(record).toMatchObject({
      fileName: 'big.bin',
      size: SIZE,
      lastModified: file.lastModified,
      parentId: PARENT,
    });
    expect(record?.blocklist).toHaveLength(6);
    release();
    await settle(service);
    expect(service.jobs[0]?.state).toBe('done');
    expect(await w.records.list('account-a')).toEqual([]);
  });

  it('removes the record when the upload is cancelled', async () => {
    // Arrange
    const w = world();
    // A block on the wire that ends when its request is aborted, as a real fetch does.
    w.server.faults.push((request, route) =>
      route === BLOCK_PUT
        ? new Promise<Response>((_resolve, reject) => rejectWhenAborted(request.signal, reject))
        : undefined,
    );
    const service = w.tab();
    await service.enqueueUpload([patternFile(SIZE, 'big.bin')], PARENT);
    await vi.waitFor(async () => expect(await w.records.list('account-a')).toHaveLength(1));

    // Act
    service.cancelUpload(service.jobs[0]?.id ?? '');
    await settle(service);

    // Assert
    expect(service.jobs[0]?.state).toBe('cancelled');
    await vi.waitFor(async () => expect(await w.records.list('account-a')).toEqual([]));
  });
});

describe('resuming after the tab closed', () => {
  it('lists the stopped upload in the new tab and sends only the missing blocks under the same client key', async () => {
    // Arrange
    const w = world();
    const file = patternFile(SIZE, 'big.bin');
    const run1: number[] = [];
    await stopTab(w, file, 3, run1);
    const run2: number[] = [];
    w.server.faults.push((request, route) => {
      if (route === BLOCK_PUT) run2.push(blockIndex(request));
      return undefined;
    });
    const second = w.tab();

    // Act
    await second.loadStopped();
    const row = second.jobs[0] as JobProgress;
    const result = await second.resumeUpload(file, row.id);
    await settle(second);

    // Assert
    expect(row).toMatchObject({ name: 'big.bin', state: 'done' });
    expect(result.ok && result.value.kind).toBe('Resumed');
    const resent = result.ok && result.value.kind === 'Resumed' ? result.value.resentBytes : -1;
    expect(resent).toBeLessThanOrEqual(10 * MIB);
    const sentAgain = run2.filter((i) => run1.includes(i));
    const sentAgainBytes = sentAgain.reduce((sum, i) => sum + (i === 5 ? 123 : 2 * MIB), 0);
    expect(sentAgainBytes).toBeLessThanOrEqual(resent);
    expect(run2).toEqual([3, 4, 5]);
    expect(w.server.requests.filter((r) => r === 'POST /api/v1/uploads')).toHaveLength(1);
    expect(w.server.nodes).toHaveLength(1);
    expect(
      sameBytes(w.server.contents.get(w.server.nodes[0]?.nodeId ?? ''), patternBytes(SIZE)),
    ).toBe(true);
    expect(await w.records.list('account-a')).toEqual([]);
  });

  it.each([
    { what: 'a changed modified time', change: { lastModified: 1_800_000_000_000 }, size: SIZE },
    { what: 'another size', change: { lastModified: 1_700_000_000_000 }, size: SIZE + 5 },
  ])('restarts from the start with $what and aborts the old session', async ({ change, size }) => {
    // Arrange
    const w = world();
    await stopTab(w, patternFile(SIZE, 'big.bin'));
    const oldSession = [...w.server.sessions.keys()][0];
    const changed = new File([patternBytes(size)], 'big.bin', change);
    const second = w.tab();
    await second.loadStopped();

    // Act
    const result = await second.resumeUpload(changed, second.jobs[0]?.id ?? '');
    await settle(second);

    // Assert
    expect(result).toEqual({
      ok: true,
      value: { kind: 'Restarted', reason: 'ChangedSinceLastTime' },
    });
    expect(second.jobs[0]?.note).toMatch(/changed since last time/);
    expect(w.server.deleted).toContain(oldSession);
    expect(w.server.requests.filter((r) => r === 'POST /api/v1/uploads')).toHaveLength(2);
    expect(w.server.nodes).toHaveLength(1);
    expect(w.server.nodes[0]?.size).toBe(size);
    expect(await w.records.list('account-a')).toEqual([]);
  });

  it('says UploadSessionGone when the session expired, removes the record, and lets a retry upload again', async () => {
    // Arrange
    const w = world();
    const file = patternFile(SIZE, 'big.bin');
    await stopTab(w, file);
    w.server.sessions.clear();
    const second = w.tab();
    await second.loadStopped();

    // Act
    const result = await second.resumeUpload(file, second.jobs[0]?.id ?? '');

    // Assert
    expect(result).toMatchObject({ ok: false, error: { code: 'UploadSessionGone' } });
    expect(second.jobs[0]).toMatchObject({ state: 'failed', error: 'UploadSessionGone' });
    expect(second.jobs[0]?.errorText).toMatch(/expired/);
    await vi.waitFor(async () => expect(await w.records.list('account-a')).toEqual([]));
    second.retryUpload(second.jobs[0]?.id ?? '');
    await settle(second);
    expect(second.jobs[0]?.state).toBe('done');
    expect(w.server.nodes).toHaveLength(1);
  });

  it('answers FileUnreadable when the selected file cannot be read, and keeps the stopped row', async () => {
    // Arrange
    const w = world();
    const file = patternFile(SIZE, 'big.bin');
    await stopTab(w, file);
    const second = w.tab();
    await second.loadStopped();
    const unreadable = patternFile(SIZE, 'big.bin');
    unreadable.slice = () =>
      ({ arrayBuffer: () => Promise.reject(new DOMException('gone', 'NotFoundError')) }) as Blob;

    // Act
    const result = await second.resumeUpload(unreadable, second.jobs[0]?.id ?? '');

    // Assert
    expect(result).toMatchObject({ ok: false, error: { code: 'FileUnreadable' } });
    expect(second.jobs[0]?.state).toBe('stopped');
    expect(await w.records.list('account-a')).toHaveLength(1);
  });

  it('removes the record and the session when a stopped upload is cancelled', async () => {
    // Arrange
    const w = world();
    await stopTab(w, patternFile(SIZE, 'big.bin'));
    const sessionId = [...w.server.sessions.keys()][0];
    const second = w.tab();
    await second.loadStopped();

    // Act
    second.cancelUpload(second.jobs[0]?.id ?? '');

    // Assert
    expect(second.jobs[0]?.state).toBe('cancelled');
    await vi.waitFor(() => expect(w.server.deleted).toContain(sessionId));
    expect(await w.records.list('account-a')).toEqual([]);
  });
});

describe('an upload that will not continue', () => {
  it('removes the record and the session when a re-selected upload is cancelled while it waits in the queue', async () => {
    // Arrange
    const w = world();
    const first = patternFile(SIZE, 'one.bin');
    const second = patternFile(SIZE, 'two.bin');
    await stopTab(w, first);
    const secondRecord = {
      ...(await w.records.list('account-a'))[0],
      fileName: 'two.bin',
    } as ResumeRecord;
    await w.records.put('account-a', {
      ...secondRecord,
      clientKey: '0190bbbb-0000-7000-8000-0000000000aa',
      sessionId: '0190aaaa-0000-7000-8000-0000000000aa',
    });
    const other = w.tab();
    await other.loadStopped();
    stallBlocksFrom(w.server, 0, []);
    const idOf = (name: string) => other.jobs.find((j) => j.name === name)?.id ?? '';
    const running = other.resumeUpload(first, idOf('one.bin'));
    const waiting = other.resumeUpload(second, idOf('two.bin'));
    await vi.waitFor(() =>
      expect(other.jobs.find((j) => j.name === 'two.bin')?.state).toBe('queued'),
    );

    // Act
    other.cancelUpload(idOf('two.bin'));

    // Assert
    expect(other.jobs.find((j) => j.name === 'two.bin')?.state).toBe('cancelled');
    expect((await waiting).ok).toBe(false);
    expect(await w.records.list('account-a')).toHaveLength(1);
    void running;
  });

  it('removes the record when a failed large upload is dismissed', async () => {
    // Arrange
    const w = world();
    w.server.faults.push((_request, route) =>
      route === BLOCK_PUT
        ? Promise.resolve(
            new Response(JSON.stringify({ code: 'forbidden', title: 'Forbidden' }), {
              status: 403,
              headers: { 'content-type': 'application/problem+json' },
            }),
          )
        : undefined,
    );
    const service = w.tab();
    await service.enqueueUpload([patternFile(SIZE, 'big.bin')], PARENT);
    await settle(service);
    expect(service.jobs[0]?.state).toBe('failed');
    expect(await w.records.list('account-a')).toHaveLength(1);

    // Act
    service.dismiss(service.jobs[0]?.id ?? '');

    // Assert
    await vi.waitFor(async () => expect(await w.records.list('account-a')).toEqual([]));
  });
});

describe('stopped uploads belong to one account', () => {
  it('lists both stopped uploads for their owner and none for another account', async () => {
    // Arrange
    const w = world();
    const records: ResumeRecord[] = ['one.bin', 'two.bin'].map((name, i) => ({
      fileName: name,
      size: SIZE,
      lastModified: 1_700_000_000_000,
      sessionId: `0190aaaa-0000-7000-8000-00000000000${i + 1}`,
      clientKey: `0190bbbb-0000-7000-8000-00000000000${i + 1}`,
      blocklist: [],
      parentId: PARENT,
    }));
    for (const record of records) await w.records.put('account-a', record);
    const owner = w.tab('account-a');
    const other = w.tab('account-b');

    // Act
    await owner.loadStopped();
    await other.loadStopped();

    // Assert
    expect(owner.jobs.map((j) => [j.name, j.state, j.errorText])).toEqual([
      ['one.bin', 'stopped', STOPPED_TEXT.PageHidden],
      ['two.bin', 'stopped', STOPPED_TEXT.PageHidden],
    ]);
    expect(other.jobs).toEqual([]);
  });

  it('does not show the rows of an account that was signed in earlier in the same tab', async () => {
    // Arrange
    const w = world();
    await w.records.put('account-a', {
      fileName: 'one.bin',
      size: SIZE,
      lastModified: 1,
      sessionId: '0190aaaa-0000-7000-8000-000000000001',
      clientKey: '0190bbbb-0000-7000-8000-000000000001',
      blocklist: [],
      parentId: PARENT,
    });
    let account = 'account-a';
    const service = new TransferService({
      api: pageApi(w.server),
      connect: inProcessWorker(w.server).connect,
      resume: w.records,
      accountId: () => account,
    });
    await service.loadStopped();

    // Act
    account = 'account-b';
    await service.loadStopped();

    // Assert
    expect(service.jobs).toEqual([]);
  });
});

describe('a file edited while it uploads', () => {
  it('stops the upload and restarts it with the new content when the person selects the file again', async () => {
    // Arrange: after the third block the browser refuses further reads, as it does for a file replaced on disk.
    const w = world();
    const original = patternFile(SIZE, 'big.bin');
    let reads = 0;
    const slice = original.slice.bind(original);
    original.slice = (start?: number, end?: number) => {
      reads += 1;
      // The hash pass reads six blocks; the sends then read from the seventh read on.
      return reads > 9
        ? ({
            arrayBuffer: () => Promise.reject(new DOMException('changed', 'NotReadableError')),
          } as Blob)
        : slice(start, end);
    };
    const service = w.tab();

    // Act
    await service.enqueueUpload([original], PARENT);
    await settle(service);
    const stoppedRow = { ...service.jobs[0] };
    const edited = new File([patternBytes(SIZE).map((b) => b ^ 0xff)], 'big.bin', {
      lastModified: 1_800_000_000_000,
    });
    const result = await service.resumeUpload(edited, service.jobs[0]?.id ?? '');
    await settle(service);

    // Assert
    expect(stoppedRow.state).toBe('stopped');
    expect(stoppedRow.errorText).toBe(STOPPED_TEXT.FileUnreadable);
    expect(result).toEqual({
      ok: true,
      value: { kind: 'Restarted', reason: 'ChangedSinceLastTime' },
    });
    expect(w.server.nodes).toHaveLength(1);
    expect(
      sameBytes(
        w.server.contents.get(w.server.nodes[0]?.nodeId ?? ''),
        patternBytes(SIZE).map((b) => b ^ 0xff),
      ),
    ).toBe(true);
    expect(await w.records.list('account-a')).toEqual([]);
  });

  it('stops the upload as FileChanged when the file is edited just before the commit', async () => {
    // Arrange
    const w = world();
    const file = patternFile(SIZE, 'big.bin');
    let reads = 0;
    const slice = file.slice.bind(file);
    file.slice = (start?: number, end?: number) => {
      reads += 1;
      // Six reads hash the blocks and six send them; the check before commit is the thirteenth.
      return reads > 12
        ? ({
            arrayBuffer: () => Promise.reject(new DOMException('changed', 'NotReadableError')),
          } as Blob)
        : slice(start, end);
    };
    const service = w.tab();

    // Act
    await service.enqueueUpload([file], PARENT);
    await settle(service);

    // Assert
    expect(service.jobs[0]).toMatchObject({
      state: 'stopped',
      errorText: STOPPED_TEXT.FileChanged,
    });
    expect(w.server.nodes).toEqual([]);
    expect(await w.records.list('account-a')).toHaveLength(1);
  });
});

describe('a page that is hidden or suspended', () => {
  it('shows a running large upload as stopped, keeps its record, and fails a small one that can be retried', async () => {
    // Arrange
    const w = world();
    w.server.faults.push((_request, route) =>
      route === BLOCK_PUT || route === 'PUT /api/v1/staged-uploads/:id'
        ? new Promise<Response>(() => undefined)
        : undefined,
    );
    const service = w.tab();
    await service.enqueueUpload(
      [patternFile(SIZE, 'big.bin'), patternFile(100, 'small.bin')],
      PARENT,
    );
    await vi.waitFor(async () => expect(await w.records.list('account-a')).toHaveLength(1));

    // Act
    service.suspend();

    // Assert
    expect(service.jobs.map((j) => [j.name, j.state])).toEqual([
      ['big.bin', 'stopped'],
      ['small.bin', 'failed'],
    ]);
    expect(service.jobs[0]?.errorText).toBe(STOPPED_TEXT.PageHidden);
    expect(await w.records.list('account-a')).toHaveLength(1);
  });
});

describe('blocked storage', () => {
  it('still uploads a large file, offers no resume, and says so once', async () => {
    // Arrange
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    const w = world();
    const onUnavailable = vi.fn();
    const blocked = new ResumeStore(onUnavailable, () => undefined);
    const service = w.tab('account-a', blocked);

    // Act
    await service.enqueueUpload([patternFile(SIZE, 'big.bin')], PARENT);
    await settle(service);
    await service.loadStopped();

    // Assert
    expect(service.jobs.map((j) => j.state)).toEqual(['done']);
    expect(w.server.nodes).toHaveLength(1);
    expect(onUnavailable).toHaveBeenCalledTimes(1);
    expect(service.jobs.filter((j) => j.state === 'stopped')).toEqual([]);
  });
});
