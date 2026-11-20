import { afterEach, describe, expect, it, vi } from 'vitest';
import { patternFile, MIB } from '../../../tests/support/files';
import { problem } from '../../../tests/support/http';
import { inProcessWorker, pageApi } from '../../../tests/support/inProcessWorker';
import { UploadServer, type ServerOptions } from '../../../tests/support/uploadServer';
import type { ApiError } from '$lib/api';
import { MAX_SELECTION_FILES, TransferService, type JobProgress } from '.';

const PARENT = '0190aaaa-0000-7000-8000-00000000f001';

function setup(options: ServerOptions = {}) {
  const server = new UploadServer(options);
  const worker = inProcessWorker(server);
  const api = pageApi(server);
  const reported: ApiError[] = [];
  api.setErrorHandler((error) => reported.push(error));
  const service = new TransferService({ api, connect: worker.connect, batchRetryMs: 0 });
  return { server, service, worker, reported };
}

const tiny = (count: number) =>
  Array.from({ length: count }, (_, i) => new File([new Uint8Array((i % 9) + 1)], `file-${i}.bin`));

const finished = (jobs: JobProgress[]) =>
  jobs.every((j) => j.state === 'done' || j.state === 'failed' || j.state === 'cancelled');

async function settle(service: TransferService) {
  await vi.waitFor(() => expect(finished(service.jobs)).toBe(true), { timeout: 15_000 });
}

afterEach(() => vi.unstubAllGlobals());

describe('the transfer service: routing and batches', () => {
  it('stages files up to 2 MiB and commits them in one batch, and sends 2 MiB + 1 byte down the block path', async () => {
    // Arrange
    const { service, server } = setup();
    const files = [
      patternFile(2 * MIB - 1, 'a'),
      patternFile(2 * MIB, 'b'),
      patternFile(2 * MIB + 1, 'c'),
    ];

    // Act
    const result = await service.enqueueUpload(files, PARENT);
    await settle(service);

    // Assert
    expect(result.ok).toBe(true);
    expect(server.batches).toHaveLength(1);
    expect(server.batches[0]?.entries).toBe(2);
    expect(server.requests.filter((r) => r === 'POST /api/v1/uploads')).toHaveLength(1);
    expect(server.nodes.map((n) => n.name).sort()).toEqual(['a', 'b', 'c']);
    expect(service.jobs.map((j) => j.state)).toEqual(['done', 'done', 'done']);
  });

  it('never sends a batch above batch_max_entries and ends with exactly the files that were picked', async () => {
    // Arrange
    const { service, server } = setup({ batchMaxEntries: 100 });

    // Act
    await service.enqueueUpload(tiny(300), PARENT);
    await settle(service);

    // Assert
    expect(Math.max(...server.batches.map((b) => b.entries))).toBeLessThanOrEqual(100);
    expect(server.nodes).toHaveLength(300);
    expect(new Set(server.nodes.map((n) => n.name)).size).toBe(300);
  });

  it('retries a batch whose answer was lost with the same commit ID and creates no duplicates', async () => {
    // Arrange
    const { service, server } = setup({ batchMaxEntries: 100 });
    let cut = true;
    server.faults.push((request, route) => {
      if (route !== 'POST /api/v1/commits:batch' || !cut) return undefined;
      cut = false;
      void server.fetch(request.clone()).then(() => undefined);
      throw new TypeError('fetch failed');
    });

    // Act
    await service.enqueueUpload(tiny(150), PARENT);
    await settle(service);

    // Assert
    const ids = server.batches.map((b) => b.commitId);
    expect(ids[0]).toBe(ids[1]);
    expect(server.nodes).toHaveLength(150);
    expect(service.jobs.every((j) => j.state === 'done')).toBe(true);
  });

  it('commits what is staged, then stages the rest, when the server answers StagedCapReached', async () => {
    // Arrange
    const { service, server } = setup({
      maxStagedPerAccount: 40,
      advertisedStagedMax: 1000,
      batchMaxEntries: 500,
    });

    // Act
    await service.enqueueUpload(tiny(120), PARENT);
    await settle(service);

    // Assert
    expect(server.nodes).toHaveLength(120);
    expect(service.jobs.every((j) => j.state === 'done')).toBe(true);
    expect(server.batches.length).toBeGreaterThan(1);
  });

  it('refuses a selection above the limit with TooManyFiles and its message, and sends nothing', async () => {
    // Arrange
    const { service, server } = setup();
    const files = Array.from({ length: MAX_SELECTION_FILES + 1 }, () => new File([], 'x'));

    // Act
    const result = await service.enqueueUpload(files, PARENT);

    // Assert
    expect(result).toEqual({
      ok: false,
      error: {
        code: 'TooManyFiles',
        text: 'This selection has 100,001 files. The web client takes at most 100,000 files at once. For larger folders use the desktop client.',
      },
    });
    expect(server.requests).toEqual([]);
    expect(service.jobs).toEqual([]);
  });

  it('refuses every upload when the server block size is not 2 MiB', async () => {
    // Arrange
    const { service, server } = setup();
    server.faults.push((_r, route) =>
      route === 'GET /api/v1/server-info'
        ? Response.json({
            block_size: 4 * MIB,
            max_lanes: 4,
            small_file_max: 2 * MIB,
            batch_max_entries: 100,
            max_staged_per_account: 100,
            longpoll_max_wait: 1,
            min_client_version: '0.0.0',
            api_versions: ['v1'],
          })
        : undefined,
    );

    // Act
    const result = await service.enqueueUpload(tiny(1), PARENT);

    // Assert
    expect(result.ok).toBe(false);
    expect(server.requests).toEqual(['GET /api/v1/server-info']);
  });
});

describe('the transfer service: errors, cancel, retry', () => {
  it('shows each failed job its own message while the other jobs finish', async () => {
    // Arrange
    const { service, server } = setup();
    const files = [
      new File([new Uint8Array(5)], 'ok-1'),
      new File([new Uint8Array(5)], '<bad>'),
      new File([new Uint8Array(5)], 'ok-2'),
    ];

    // Act
    await service.enqueueUpload(files, PARENT);
    await settle(service);

    // Assert
    expect(service.jobs.map((j) => [j.name, j.state, j.error])).toEqual([
      ['ok-1', 'done', undefined],
      ['<bad>', 'failed', 'InvalidName'],
      ['ok-2', 'done', undefined],
    ]);
    expect(service.jobs[1]?.errorText).toBe('This file name is not allowed.');
    expect(server.nodes.map((n) => n.name)).toEqual(['ok-1', 'ok-2']);
  });

  it('types a refused session as QuotaExceeded and hands the 507 to the app-level notices', async () => {
    // Arrange
    const { service, server, reported } = setup();
    server.faults.push((_r, route) =>
      route === 'POST /api/v1/uploads' ? problem(507, 'quota_exceeded') : undefined,
    );

    // Act
    await service.enqueueUpload([patternFile(2 * MIB + 1)], PARENT);
    await settle(service);

    // Assert
    expect(service.jobs[0]).toMatchObject({ state: 'failed', error: 'QuotaExceeded' });
    expect(reported.map((e) => e.status)).toContain(507);
  });

  it('sends nothing for a queued job that is cancelled', async () => {
    // Arrange
    const { service, server } = setup();
    const gate = holdBlocks(server);
    const ids = await service.enqueueUpload(
      [patternFile(3 * MIB, 'first'), patternFile(3 * MIB, 'second')],
      PARENT,
    );
    if (!ids.ok) throw new Error('enqueue failed');
    await vi.waitFor(() => expect(service.jobs[0]?.state).toBe('running'));

    // Act
    service.cancelUpload(ids.value[1] ?? '');
    gate.release();
    await settle(service);

    // Assert
    expect(service.jobs.map((j) => j.state)).toEqual(['done', 'cancelled']);
    expect(server.requests.filter((r) => r === 'POST /api/v1/uploads')).toHaveLength(1);
  });

  it('aborts a running large upload and its session', async () => {
    // Arrange
    const { service, server } = setup();
    const gate = holdBlocks(server, true);
    const ids = await service.enqueueUpload([patternFile(3 * MIB)], PARENT);
    if (!ids.ok) throw new Error('enqueue failed');
    await vi.waitFor(() => expect(gate.held()).toBeGreaterThan(0));

    // Act
    service.cancelUpload(ids.value[0] ?? '');
    await settle(service);

    // Assert
    expect(service.jobs[0]?.state).toBe('cancelled');
    expect(server.deleted).toHaveLength(1);
    expect(server.nodes).toEqual([]);
  });

  it('retries a failed large upload from the start with the same client key, reusing the session', async () => {
    // Arrange
    const { service, server } = setup();
    let down = true;
    server.faults.push((_r, route) =>
      down && route === 'PUT /api/v1/uploads/:id/blocks/:n'
        ? problem(503, 'overloaded')
        : undefined,
    );
    await service.enqueueUpload([patternFile(2 * MIB + 5)], PARENT);
    await settle(service);
    expect(service.jobs[0]).toMatchObject({ state: 'failed' });
    down = false;

    // Act
    service.retryUpload(service.jobs[0]?.id ?? '');
    await settle(service);

    // Assert
    expect(service.jobs[0]?.state).toBe('done');
    expect(server.nodes).toHaveLength(1);
  });

  it('tells the file browser which node a finished upload created', async () => {
    // Arrange
    const { service, server } = setup();
    const events: { nodeId: string; name: string }[] = [];
    service.onUploaded((e) => events.push({ nodeId: e.nodeId, name: e.name }));

    // Act
    await service.enqueueUpload(
      [new File([new Uint8Array(3)], 'x.txt'), patternFile(2 * MIB + 1, 'big')],
      PARENT,
    );
    await settle(service);

    // Assert
    expect(events.map((e) => e.name).sort()).toEqual(['big', 'x.txt']);
    expect(events.map((e) => e.nodeId).sort()).toEqual(server.nodes.map((n) => n.nodeId).sort());
  });

  it('drops a finished row from the panel without touching the others', async () => {
    // Arrange
    const { service } = setup();
    await service.enqueueUpload(tiny(2), PARENT);
    await settle(service);

    // Act
    service.dismiss(service.jobs[0]?.id ?? '');

    // Assert
    expect(service.jobs.map((j) => j.name)).toEqual(['file-1.bin']);
  });
});

/** Blocks every block PUT until released (or, with `forever`, until the request is aborted). */
function holdBlocks(server: UploadServer, forever = false) {
  let open: () => void = () => {};
  const gate = new Promise<void>((resolve) => (open = resolve));
  let held = 0;
  server.faults.push((request, route) => {
    if (route !== 'PUT /api/v1/uploads/:id/blocks/:n') return undefined;
    held += 1;
    return new Promise<Response>((resolve, reject) => {
      request.signal.addEventListener('abort', () =>
        reject(new DOMException('aborted', 'AbortError')),
      );
      if (!forever) void gate.then(() => resolve(server.bypass(request)));
    });
  });
  return { release: () => open(), held: () => held };
}
