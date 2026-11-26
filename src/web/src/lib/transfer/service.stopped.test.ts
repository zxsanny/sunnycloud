import { describe, expect, it, vi } from 'vitest';
import { patternBytes, patternFile, sameBytes } from '../../../tests/support/files';
import { inProcessWorker, pageApi } from '../../../tests/support/inProcessWorker';
import { STOPPED_TEXT } from './errors';
import { ResumeStore, type ResumeRecord } from './resume';
import { TransferService } from '.';
import {
  BLOCK_PUT,
  PARENT,
  SIZE,
  installResumeHooks,
  settle,
  world,
} from './service.resume.test-support';

installResumeHooks();

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
