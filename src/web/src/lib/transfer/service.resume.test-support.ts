import { IDBFactory, IDBKeyRange } from 'fake-indexeddb';
import { afterEach, beforeEach, expect, vi } from 'vitest';
import { inProcessWorker, pageApi } from '../../../tests/support/inProcessWorker';
import { UploadServer } from '../../../tests/support/uploadServer';
import { MIB } from '../../../tests/support/files';
import { ResumeStore } from './resume';
import { TransferService, type JobProgress } from '.';

export const PARENT = '0190aaaa-0000-7000-8000-00000000f001';
/** Six blocks: five of 2 MiB and a short one. */
export const SIZE = 10 * MIB + 123;
export const BLOCK_PUT = 'PUT /api/v1/uploads/:id/blocks/:n';

/** Gives every test of the calling file the `IDBKeyRange` global that fake-indexeddb needs. */
export function installResumeHooks() {
  beforeEach(() => vi.stubGlobal('IDBKeyRange', IDBKeyRange));
  afterEach(() => vi.unstubAllGlobals());
}

/** The browser (one IndexedDB) and the server; a tab is a `TransferService` with its own Worker. */
export function world(account = 'account-a') {
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

const done = (jobs: JobProgress[]) =>
  jobs.every((j) => ['done', 'failed', 'cancelled', 'stopped'].includes(j.state));
export const settle = (service: TransferService) =>
  vi.waitFor(() => expect(done(service.jobs)).toBe(true), { timeout: 15_000 });
