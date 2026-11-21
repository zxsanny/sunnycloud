import { type Api, type ServerLimits, isTransientError } from '$lib/api';
import {
  type UploadError,
  UPLOAD_ERROR_TEXT,
  tooManyFilesText,
  uploadErrorOf,
  uploadErrorOfCode,
} from './errors';
import type { BatchEntryResult, Failure, FromWorker, ToWorker } from './protocol';

/** The few members of a Worker the service uses; Node tests give it an in-process engine instead. */
export interface WorkerLike {
  postMessage(message: ToWorker): void;
  onmessage: ((event: { data: FromWorker }) => void) | null;
  onerror: ((event: unknown) => void) | null;
  terminate(): void;
}

export interface JobProgress {
  id: string;
  name: string;
  parentId: string;
  size: number;
  state: 'queued' | 'running' | 'committing' | 'done' | 'failed' | 'cancelled';
  /** `preparing` is the hash pass of a large file, before any byte is sent. */
  phase: 'preparing' | 'sending' | null;
  bytesDone: number;
  lanes: number;
  error?: UploadError;
  errorText?: string;
}

export type EnqueueResult =
  { ok: true; value: string[] } | { ok: false; error: { code: UploadError; text: string } };

interface Job {
  id: string;
  file: File | null;
  parentId: string;
  name: string;
  size: number;
  lastModified: number;
  small: boolean;
  stagedKey?: string;
  clientKey?: string;
}

interface BatchInFlight {
  id: string;
  jobs: Job[];
  commitId?: string;
  attempts: number;
}

/** `web.max_selection_files`: a selection above it is refused before anything is queued. */
export const MAX_SELECTION_FILES = 100_000;
/** Small files staged at once. */
const STAGE_PARALLEL = 6;
/** Large files in flight at once; each already uses all its lanes. */
const LARGE_PARALLEL = 1;
const BATCH_ATTEMPTS = 3;
const BATCH_RETRY_MS = 1000;

export interface TransferDeps {
  api: Api;
  connect: () => WorkerLike;
  /** Delay unit between attempts to send a batch again; tests set 0. */
  batchRetryMs?: number;
}

/**
 * The one facade pages use for uploads (contract transfer_service.md). Routing, queueing and the progress rows
 * live here; hashing and every upload request happen in the module Worker.
 */
export class TransferService {
  /** One row per job, in the order they were enqueued. */
  jobs = $state<JobProgress[]>([]);

  // Internal bookkeeping that no template reads, so plain collections (not SvelteMap) are right.
  /* eslint-disable svelte/prefer-svelte-reactivity */
  private readonly internal = new Map<string, Job>();
  private readonly rows = new Map<string, JobProgress>();
  private readonly queuedSmall: Job[] = [];
  private readonly queuedLarge: Job[] = [];
  private readonly pending: Job[] = [];
  /* eslint-enable svelte/prefer-svelte-reactivity */
  private staging = 0;
  private largeActive = 0;
  private batch: BatchInFlight | null = null;
  private capHold = false;
  private counter = 0;
  private limits: ServerLimits | null = null;
  private worker: WorkerLike | null = null;
  private ready: Promise<void> | null = null;

  constructor(private readonly deps: TransferDeps) {}

  async enqueueUpload(files: File[], parentId: string): Promise<EnqueueResult> {
    if (files.length > MAX_SELECTION_FILES) {
      return refuse('TooManyFiles', tooManyFilesText(files.length, MAX_SELECTION_FILES));
    }
    if (this.limits === null) {
      const loaded = await this.deps.api.getServerLimits();
      // The failure itself reached the app-level notices through the API client.
      if (!loaded.ok) return refuse('Failed', UPLOAD_ERROR_TEXT.Failed);
      this.limits = loaded.value;
    }
    if (!this.limits.uploadsEnabled) {
      return refuse(
        'Failed',
        'Uploads are switched off: this server uses a block size the web app does not support.',
      );
    }
    const ids: string[] = [];
    for (const file of files) {
      const job: Job = {
        id: `up-${(this.counter += 1)}`,
        file,
        parentId,
        name: file.name,
        size: file.size,
        lastModified: file.lastModified,
        small: file.size <= this.limits.smallFileMax,
      };
      this.internal.set(job.id, job);
      this.jobs.push({
        id: job.id,
        name: job.name,
        parentId,
        size: job.size,
        state: 'queued',
        phase: null,
        bytesDone: 0,
        lanes: 0,
      });
      this.rows.set(job.id, this.jobs[this.jobs.length - 1] as JobProgress);
      (job.small ? this.queuedSmall : this.queuedLarge).push(job);
      ids.push(job.id);
    }
    this.pump();
    return { ok: true, value: ids };
  }

  /** Queued: nothing is sent. Running: the Worker aborts its requests (a large upload also aborts its session). */
  cancelUpload(id: string) {
    const job = this.internal.get(id);
    const row = this.rows.get(id);
    if (!job || !row) return;
    if (row.state === 'queued') {
      remove(this.queuedSmall, job);
      remove(this.queuedLarge, job);
      this.finish(job, row, 'cancelled');
    } else if (row.state === 'committing' && this.pending.includes(job)) {
      remove(this.pending, job);
      this.finish(job, row, 'cancelled');
    } else if (row.state === 'running') {
      this.send({ type: 'cancel', job: id });
    }
    // A job inside a batch request that is already on the wire finishes with that request.
  }

  /** A failed job starts again from the beginning: same client key, so a session that is still alive is reused. */
  retryUpload(id: string) {
    const job = this.internal.get(id);
    const row = this.rows.get(id);
    if (!job || !row || row.state !== 'failed' || job.file === null) return;
    row.state = 'queued';
    row.error = undefined;
    row.errorText = undefined;
    row.bytesDone = 0;
    row.phase = null;
    (job.small ? this.queuedSmall : this.queuedLarge).push(job);
    this.pump();
  }

  /** Removes a finished row from the panel. */
  dismiss(id: string) {
    const row = this.rows.get(id);
    if (!row || row.state === 'queued' || row.state === 'running' || row.state === 'committing')
      return;
    this.jobs = this.jobs.filter((r) => r.id !== id);
    this.rows.delete(id);
    this.internal.delete(id);
  }

  private pump() {
    const limits = this.limits;
    if (limits === null || this.deps.api.upgradeRequired()) return;
    while (
      !this.capHold &&
      this.staging < STAGE_PARALLEL &&
      this.staging + this.pending.length < limits.maxStagedPerAccount
    ) {
      const job = this.queuedSmall.shift();
      if (!job) break;
      this.staging += 1;
      this.setRunning(job);
      this.send({ type: 'stage', job: job.id, file: job.file as File, stagedKey: job.stagedKey });
    }
    if (this.largeActive < LARGE_PARALLEL) {
      const job = this.queuedLarge.shift();
      if (job) {
        this.largeActive += 1;
        this.setRunning(job);
        this.send({
          type: 'upload',
          job: job.id,
          file: job.file as File,
          parentId: job.parentId,
          name: job.name,
          lanes: limits.lanes,
          clientKey: job.clientKey,
          clientMtime: job.lastModified,
        });
      }
    }
    this.flush();
  }

  /** Commits what is staged once a batch is full, no more small files are coming, or the staging cap holds. */
  private flush() {
    const limits = this.limits;
    if (limits === null || this.batch !== null || this.pending.length === 0) return;
    const moreComing = this.queuedSmall.length > 0 || this.staging > 0;
    if (this.pending.length < limits.batchMaxEntries && moreComing && !this.capHold) return;
    const jobs = this.pending.splice(0, limits.batchMaxEntries);
    this.batch = { id: `batch-${(this.counter += 1)}`, jobs, attempts: 1 };
    this.sendBatch(this.batch);
  }

  private sendBatch(batch: BatchInFlight) {
    this.send({
      type: 'batch',
      batch: batch.id,
      commitId: batch.commitId,
      entries: batch.jobs.map((job) => ({
        stagedKey: job.stagedKey as string,
        parentId: job.parentId,
        name: job.name,
        clientMtime: job.lastModified,
      })),
    });
  }

  private send(message: ToWorker) {
    if (this.worker === null) {
      const worker = this.deps.connect();
      this.worker = worker;
      this.ready = new Promise((resolve, reject) => {
        worker.onmessage = (event) => {
          if (event.data.type === 'ready') resolve();
          else if (event.data.type === 'fatal') reject(new Error(event.data.message));
          else this.receive(event.data);
        };
        worker.onerror = () => reject(new Error('The transfer worker failed to start.'));
      });
      this.ready.catch((cause: unknown) => {
        console.error('The transfer worker is not available.', cause);
        this.failAllActive();
      });
    }
    const worker = this.worker;
    void this.ready?.then(() => worker.postMessage(message)).catch(() => undefined);
  }

  private receive(m: FromWorker) {
    switch (m.type) {
      case 'staged': {
        const job = this.internal.get(m.job);
        if (!job) break;
        job.stagedKey = m.stagedKey;
        this.staging -= 1;
        this.pending.push(job);
        const row = this.rows.get(m.job);
        if (row) row.state = 'committing';
        break;
      }
      case 'stageFailed': {
        const job = this.internal.get(m.job);
        if (!job) break;
        this.staging -= 1;
        if (m.stagedKey) job.stagedKey = m.stagedKey;
        const capped = uploadErrorOf(m.failure) === 'StagedCapReached';
        if (capped && (this.pending.length > 0 || this.batch !== null)) {
          // Commit what is staged first, then stage the rest.
          this.capHold = true;
          const row = this.rows.get(m.job);
          if (row) row.state = 'queued';
          this.queuedSmall.unshift(job);
        } else {
          this.fail(job, m.failure);
        }
        break;
      }
      case 'batchDone':
        this.batchDone(m.entries);
        break;
      case 'batchFailed':
        this.batchFailed(m.commitId, m.failure);
        break;
      case 'progress': {
        const row = this.rows.get(m.job);
        if (!row) break;
        row.phase = m.phase;
        row.bytesDone = m.bytesDone;
        row.lanes = m.lanes;
        break;
      }
      case 'done': {
        const job = this.internal.get(m.job);
        const row = this.rows.get(m.job);
        if (!job || !row) break;
        this.largeActive -= 1;
        row.bytesDone = row.size;
        row.lanes = 0;
        this.finish(job, row, 'done');
        break;
      }
      case 'failed': {
        const job = this.internal.get(m.job);
        if (!job) break;
        this.largeActive -= 1;
        if (m.clientKey) job.clientKey = m.clientKey;
        this.fail(job, m.failure);
        break;
      }
      case 'cancelled': {
        const job = this.internal.get(m.job);
        const row = this.rows.get(m.job);
        if (!job || !row) break;
        if (job.small) this.staging -= 1;
        else this.largeActive -= 1;
        this.finish(job, row, 'cancelled');
        break;
      }
      case 'ready':
      case 'fatal':
        break;
    }
    this.pump();
  }

  private batchDone(entries: BatchEntryResult[]) {
    const batch = this.batch;
    if (!batch) return;
    this.batch = null;
    this.capHold = false;
    batch.jobs.forEach((job, index) => {
      const row = this.rows.get(job.id);
      const entry = entries[index];
      if (!row) return;
      if (entry?.status === 'committed') {
        row.bytesDone = row.size;
        this.finish(job, row, 'done');
      } else {
        // A staged key is single-use once committed or refused; a retry stages the file again.
        job.stagedKey = undefined;
        this.failWith(job, uploadErrorOfCode(entry?.status === 'error' ? entry.code : 'internal'));
      }
    });
  }

  private batchFailed(commitId: string, failure: Failure) {
    const batch = this.batch;
    if (!batch) return;
    batch.commitId = commitId;
    if (failure.kind === 'api') this.deps.api.report(failure.error);
    if (
      failure.kind === 'api' &&
      isTransientError(failure.error) &&
      batch.attempts < BATCH_ATTEMPTS
    ) {
      batch.attempts += 1;
      // The same commit ID, so a batch that did commit answers with its stored result and nothing is duplicated.
      setTimeout(
        () => this.sendBatch(batch),
        (this.deps.batchRetryMs ?? BATCH_RETRY_MS) * (batch.attempts - 1),
      );
      return;
    }
    this.batch = null;
    for (const job of batch.jobs) this.fail(job, failure, false);
  }

  private fail(job: Job, failure: Failure, report = true) {
    if (report && failure.kind === 'api') this.deps.api.report(failure.error);
    this.failWith(job, uploadErrorOf(failure));
  }

  private failWith(job: Job, error: UploadError) {
    const row = this.rows.get(job.id);
    if (!row) return;
    row.state = 'failed';
    row.phase = null;
    row.lanes = 0;
    row.error = error;
    row.errorText = UPLOAD_ERROR_TEXT[error];
  }

  private failAllActive() {
    for (const row of this.jobs) {
      if (row.state === 'running' || row.state === 'committing' || row.state === 'queued') {
        row.state = 'failed';
        row.error = 'Failed';
        row.errorText = UPLOAD_ERROR_TEXT.Failed;
      }
    }
  }

  private finish(job: Job, row: JobProgress, state: 'done' | 'cancelled') {
    row.state = state;
    row.phase = null;
    row.lanes = 0;
    // The page holds no File after hand-off of a finished job.
    job.file = null;
  }

  private setRunning(job: Job) {
    const row = this.rows.get(job.id);
    if (row) row.state = 'running';
  }

  /** Stops the Worker; tests and hot reload use it. */
  dispose() {
    this.worker?.terminate();
    this.worker = null;
    this.ready = null;
  }
}

function remove<T>(list: T[], item: T) {
  const at = list.indexOf(item);
  if (at !== -1) list.splice(at, 1);
}

function refuse(code: UploadError, text: string): EnqueueResult {
  return { ok: false, error: { code, text } };
}
