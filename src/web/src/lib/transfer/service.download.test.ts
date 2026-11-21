import { afterEach, describe, expect, it, vi } from 'vitest';
import { ContentServer, fileHashOf } from '../../../tests/support/contentServer';
import { MIB } from '../../../tests/support/files';
import { inProcessWorker, pageApi } from '../../../tests/support/inProcessWorker';
import { MemorySink } from '../../../tests/support/memorySink';
import { UploadServer } from '../../../tests/support/uploadServer';
import { TransferService, VERIFIED_TEXT, type DownloadEnv } from '.';
import type { DownloadSink } from './protocol';

const NODE = '0190aaaa-0000-7000-8000-00000000d001';

interface Capabilities {
  picker?: 'chosen' | 'cancelled' | 'broken';
  firefox?: boolean;
  free?: number | null;
}

function setup(capabilities: Capabilities, options: { hashVerifyMax?: number } = {}) {
  const server = new ContentServer();
  const sinks: { spec: DownloadSink; sink: MemorySink }[] = [];
  const worker = inProcessWorker(server as unknown as UploadServer, async (spec) => {
    const sink = new MemorySink();
    sinks.push({ spec, sink });
    return sink;
  });
  const saved: { source: string | File; name: string }[] = [];
  const picked: string[] = [];
  const env: DownloadEnv = {
    pickFile:
      capabilities.picker === undefined
        ? undefined
        : async (name) => {
            picked.push(name);
            if (capabilities.picker === 'cancelled') throw new DOMException('closed', 'AbortError');
            if (capabilities.picker === 'broken')
              throw new DOMException('no gesture', 'SecurityError');
            return {} as FileSystemFileHandle;
          },
    opfsVerified: () => capabilities.firefox === true,
    freeStorage: () => Promise.resolve(capabilities.free ?? null),
    saveFile: (source, name) => saved.push({ source, name }),
  };
  const service = new TransferService({
    api: pageApi(server as unknown as UploadServer),
    connect: worker.connect,
    download: env,
    hashVerifyMax: options.hashVerifyMax,
  });
  return { server, service, sinks, saved, picked, worker };
}

afterEach(() => vi.restoreAllMocks());

describe('download in Chromium: the save dialog, streamed and verified', () => {
  it('saves into the chosen file, ends as VerifiedSaved and shows the verified label', async () => {
    // Arrange
    const { server, service, sinks, saved, picked } = setup({ picker: 'chosen' });
    const file = server.add(NODE, 9 * MIB, 'holiday.mov');

    // Act
    const outcome = await service.download({ nodeId: NODE });

    // Assert
    expect(outcome).toEqual({ kind: 'VerifiedSaved' });
    expect(picked).toEqual(['holiday.mov']);
    expect(sinks[0]?.spec.kind).toBe('picker');
    expect(fileHashOf(sinks[0]?.sink.bytes as Uint8Array)).toBe(fileHashOf(file.bytes));
    expect(sinks[0]?.sink.committed).toBe(true);
    expect(saved).toEqual([]);
    expect(service.jobs[0]).toMatchObject({
      direction: 'download',
      name: 'holiday.mov',
      state: 'done',
      bytesDone: 9 * MIB,
      note: VERIFIED_TEXT,
    });
  });

  it('keeps the dialog above the Firefox and Safari limit: Chromium verifies at any size', async () => {
    // Arrange: a limit of 1 byte stands in for 2 GiB.
    const { server, service, sinks } = setup({ picker: 'chosen' }, { hashVerifyMax: 1 });
    server.add(NODE, 3 * MIB);

    // Act
    const outcome = await service.download({ nodeId: NODE });

    // Assert: 3 MiB is far above the limit of 1 byte, and Chromium still verifies.
    expect(outcome).toEqual({ kind: 'VerifiedSaved' });
    expect(sinks[0]?.spec.kind).toBe('picker');
  });

  it('ends as Cancelled when the person closes the dialog, and asks the server for no byte', async () => {
    // Arrange
    const { server, service, sinks } = setup({ picker: 'cancelled' });
    server.add(NODE, 3 * MIB);

    // Act
    const outcome = await service.download({ nodeId: NODE });

    // Assert
    expect(outcome).toEqual({ kind: 'Failed', reason: 'Cancelled' });
    expect(service.jobs[0]?.state).toBe('cancelled');
    expect(sinks).toEqual([]);
    expect(server.requests.some((r) => r.range !== null)).toBe(false);
  });

  it('ends as SaveFailed, with the error logged, when the dialog cannot open', async () => {
    // Arrange
    const { server, service } = setup({ picker: 'broken' });
    server.add(NODE, 3 * MIB);
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});

    // Act
    const outcome = await service.download({ nodeId: NODE });

    // Assert
    expect(outcome).toEqual({ kind: 'Failed', reason: 'SaveFailed' });
    expect(error).toHaveBeenCalledOnce();
  });

  it('never calls a corrupt copy good: HashMismatch, the file discarded, the row failed', async () => {
    // Arrange
    const { server, service, sinks, saved } = setup({ picker: 'chosen' });
    server.add(NODE, 5 * MIB);
    server.fault = { flipAt: 3 * MIB };

    // Act
    const outcome = await service.download({ nodeId: NODE });

    // Assert
    expect(outcome).toEqual({ kind: 'Failed', reason: 'HashMismatch' });
    expect(sinks[0]?.sink.committed).toBe(false);
    expect(sinks[0]?.sink.discarded).toBe(true);
    expect(saved).toEqual([]);
    expect(service.jobs[0]?.state).toBe('failed');
    expect(service.jobs[0]?.note).toBeUndefined();
    expect(service.jobs[0]?.errorText).toMatch(/did not match/);
  });
});

describe('download in Firefox: the private browser file, verified up to the limit', () => {
  it('verifies, then hands the finished file to the browser under its own name', async () => {
    // Arrange
    const { server, service, sinks, saved } = setup({ firefox: true });
    const file = server.add(NODE, 7 * MIB, 'photos.tar');

    // Act
    const outcome = await service.download({ nodeId: NODE });

    // Assert
    expect(outcome).toEqual({ kind: 'VerifiedSaved' });
    expect(sinks[0]?.spec).toEqual({
      kind: 'opfs',
      name: expect.stringMatching(/^dl-[0-9a-f-]{36}$/),
    });
    expect(saved).toHaveLength(1);
    expect(saved[0]?.name).toBe('photos.tar');
    expect(saved[0]?.source).toBeInstanceOf(File);
    expect(fileHashOf(sinks[0]?.sink.bytes as Uint8Array)).toBe(fileHashOf(file.bytes));
  });

  it('hands nothing to the browser when the hashes differ', async () => {
    // Arrange
    const { server, service, saved } = setup({ firefox: true });
    server.add(NODE, 5 * MIB);
    server.fault = { flipAt: 100 };

    // Act
    const outcome = await service.download({ nodeId: NODE });

    // Assert
    expect(outcome).toEqual({ kind: 'Failed', reason: 'HashMismatch' });
    expect(saved).toEqual([]);
  });

  it('takes the verified path at exactly the limit and the labelled path one byte above it', async () => {
    // Arrange: the limit is a config value; 4 MiB stands in for 2 GiB.
    const limit = 4 * MIB;
    const at = setup({ firefox: true }, { hashVerifyMax: limit });
    at.server.add(NODE, limit);
    const over = setup({ firefox: true }, { hashVerifyMax: limit });
    const bigger = over.server.add(NODE, limit + 1, 'backup.tar');

    // Act
    const first = await at.service.download({ nodeId: NODE });
    const second = await over.service.download({ nodeId: NODE });

    // Assert
    expect(first).toEqual({ kind: 'VerifiedSaved' });
    expect(second).toEqual({
      kind: 'SavedUnverified',
      reason: 'OverBrowserVerifyLimit',
      serverHash: fileHashOf(bigger.bytes),
    });
    expect(over.sinks).toEqual([]);
    expect(over.saved).toEqual([{ source: `/api/v1/nodes/${NODE}/content`, name: 'backup.tar' }]);
  });

  it('labels the file unverified with the server hash when the browser has no room for the private copy', async () => {
    // Arrange
    const { server, service, saved } = setup({ firefox: true, free: MIB });
    const file = server.add(NODE, 3 * MIB);

    // Act
    const outcome = await service.download({ nodeId: NODE });

    // Assert
    expect(outcome).toEqual({
      kind: 'SavedUnverified',
      reason: 'NotEnoughBrowserStorage',
      serverHash: fileHashOf(file.bytes),
    });
    expect(saved).toHaveLength(1);
  });
});

describe('download in a browser with no verified mechanism (Safari: not measured yet)', () => {
  it('hands the API URL to the browser, ends as SavedUnverified and shows the label with the hash', async () => {
    // Arrange
    const { server, service, saved, sinks } = setup({});
    const file = server.add(NODE, 3 * MIB, 'archive.zip');

    // Act
    const outcome = await service.download({ nodeId: NODE });

    // Assert
    const hash = fileHashOf(file.bytes);
    expect(outcome).toEqual({
      kind: 'SavedUnverified',
      reason: 'NoVerifiedPathInBrowser',
      serverHash: hash,
    });
    expect(saved).toEqual([{ source: `/api/v1/nodes/${NODE}/content`, name: 'archive.zip' }]);
    expect(sinks).toEqual([]);
    expect(service.jobs[0]).toMatchObject({
      state: 'done',
      note: 'Saved by the browser without a check (this browser cannot check downloads).',
      outcome: { serverHash: hash },
    });
  });

  it('labels a 3 GiB file in Firefox "over 2 GB in this browser" with the server hash, with the real limit', async () => {
    // Arrange: the metadata says 3 GiB; the native path reads no byte, so none is built.
    const { server, service, saved, sinks } = setup({ firefox: true });
    const file = server.add(NODE, 10, 'backup.tar');
    file.declaredSize = 3 * 1024 ** 3;
    file.hash = 'ab'.repeat(32);

    // Act
    const outcome = await service.download({ nodeId: NODE });

    // Assert
    expect(outcome).toEqual({
      kind: 'SavedUnverified',
      reason: 'OverBrowserVerifyLimit',
      serverHash: 'ab'.repeat(32),
    });
    expect(service.jobs[0]?.note).toBe(
      'Saved by the browser without a check (over 2 GB in this browser).',
    );
    expect(saved).toEqual([{ source: `/api/v1/nodes/${NODE}/content`, name: 'backup.tar' }]);
    expect(sinks).toEqual([]);
    expect(server.requests.some((r) => r.range !== null)).toBe(false);
  });

  it('downloads a public link with the browser, as unverified, because the link answer carries no file hash', async () => {
    // Arrange
    const { server, service, saved } = setup({ picker: 'chosen' });
    server.shares.set('tok', { name: 'shared.pdf', bytes: new Uint8Array(10) });

    // Act
    const outcome = await service.download({ shareToken: 'tok' });

    // Assert
    expect(outcome).toEqual({ kind: 'SavedUnverified', reason: 'NoServerHash', serverHash: null });
    expect(saved).toEqual([{ source: '/api/v1/public-shares/tok/content', name: 'shared.pdf' }]);
  });
});

describe('typed failures before anything is saved', () => {
  it('maps a non-current revision of a non-admin to Forbidden, with its message', async () => {
    // Arrange
    const { server, service, saved, sinks } = setup({ picker: 'chosen' });
    server.add(NODE, 3 * MIB);
    server.retired.add(`rev-${NODE}`);

    // Act
    const outcome = await service.download({ nodeId: NODE, revisionId: `rev-${NODE}` });

    // Assert
    expect(outcome).toEqual({ kind: 'Failed', reason: 'Forbidden' });
    expect(service.jobs[0]).toMatchObject({
      state: 'failed',
      errorText: 'You cannot download this item.',
    });
    expect(saved).toEqual([]);
    expect(sinks).toEqual([]);
  });

  it.each([
    { name: 'a node that is gone', setupNode: false, state: 'live' as const },
    { name: 'a deleted file', setupNode: true, state: 'tombstone' as const },
  ])('maps $name to NotFound and saves nothing', async ({ setupNode, state }) => {
    // Arrange
    const { server, service, saved, sinks } = setup({ picker: 'chosen' });
    if (setupNode) server.add(NODE, 3 * MIB).state = state;

    // Act
    const outcome = await service.download({ nodeId: NODE });

    // Assert
    expect(outcome).toEqual({ kind: 'Failed', reason: 'NotFound' });
    expect(service.jobs[0]?.errorText).toBe('This item no longer exists.');
    expect(saved).toEqual([]);
    expect(sinks).toEqual([]);
  });

  it('ends as Network when the page goes away mid-download, never as a good file', async () => {
    // Arrange
    const { server, service, saved } = setup({ picker: 'chosen' });
    server.add(NODE, 9 * MIB);
    server.fault = { hang: true };
    const pending = service.download({ nodeId: NODE });
    await vi.waitFor(() => expect(server.requests.some((r) => r.range !== null)).toBe(true));

    // Act
    service.suspend();
    const outcome = await pending;

    // Assert
    expect(outcome).toEqual({ kind: 'Failed', reason: 'Network' });
    expect(service.jobs[0]?.state).toBe('failed');
    expect(saved).toEqual([]);
  });

  it('ends as SaveFailed when the service was built without browser capabilities', async () => {
    // Arrange
    const server = new ContentServer();
    const service = new TransferService({
      api: pageApi(server as unknown as UploadServer),
      connect: inProcessWorker(server as unknown as UploadServer).connect,
    });
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});

    // Act
    const outcome = await service.download({ nodeId: NODE });

    // Assert
    expect(outcome).toEqual({ kind: 'Failed', reason: 'SaveFailed' });
    expect(error).toHaveBeenCalledOnce();
  });
});
