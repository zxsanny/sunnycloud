import { type ServerLimits, isTransientError } from '$lib/api';
import {
  UPLOAD_ERROR_TEXT,
  tooManyFilesText,
  uploadErrorOf,
  uploadErrorOfCode,
  type UploadError,
} from './errors';
import {
  BATCH_ATTEMPTS,
  BATCH_RETRY_MS,
  LARGE_PARALLEL,
  MAX_SELECTION_FILES,
  STAGE_PARALLEL,
  refuse,
  remove,
  type BatchInFlight,
  type EnqueueResult,
  type Job,
  type JobProgress,
  type Refusal,
  type TransferDeps,
  type WorkerLike,
} from './jobs';
import type { BatchEntryResult, Failure, FromWorker, ToWorker } from './protocol';
import { savedSession } from './resume';

/**
 * Routing, queueing and the progress rows of uploads; hashing and every upload request happen in the module Worker.
 * `TransferService` adds resuming a stopped upload.
 */
export class UploadQueue {
  /** One row per job, in the order they were enqueued. */
  jobs = $state<JobProgress[]>([]);

  // Internal bookkeeping that no template reads, so plain collections (not SvelteMap) are right.
  /* eslint-disable svelte/prefer-svelte-reactivity */
  protected readonly internal = new Map<string, Job>();
  protected readonly rows = new Map<string, JobProgress>();
  protected readonly queuedSmall: Job[] = [];
  protected readonly queuedLarge: Job[] = [];
  protected readonly pending: Job[] = [];
  /* eslint-enable svelte/prefer-svelte-reactivity */
  protected staging = 0;
  protected largeActive = 0;
  protected batch: BatchInFlight | null = null;
  protected capHold = false;
  protected counter = 0;
  protected limits: ServerLimits | null = null;
  protected worker: WorkerLike | null = null;
  protected ready: Promise<void> | null = null;

  constructor(protected readonly deps: TransferDeps) {}

  async enqueueUpload(files: File[], parentId: string): Promise<EnqueueResult> {
    if (files.length > MAX_SELECTION_FILES) {
      return refuse('TooManyFiles', tooManyFilesText(files.length, MAX_SELECTION_FILES));
    }
    const limits = await this.loadLimits();
    if ('ok' in limits) return limits;
    const accountId = this.deps.accountId?.() ?? null;
    const ids: string[] = [];
    for (const file of files) {
      const job: Job = {
        id: `up-${(this.counter += 1)}`,
        file,
        parentId,
        name: file.name,
        size: file.size,
        lastModified: file.lastModified,
        small: file.size <= limits.smallFileMax,
        accountId,
      };
      this.addRow(job, 'queued');
      (job.small ? this.queuedSmall : this.queuedLarge).push(job);
      ids.push(job.id);
    }
    this.pump();
    return { ok: true, value: ids };
  }

  /** The server's limits (read once), or the refusal that ends the call. */
  protected async loadLimits(): Promise<ServerLimits | Refusal> {
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
    return this.limits;
  }

  protected addRow(job: Job, state: JobProgress['state']) {
    this.internal.set(job.id, job);
    this.jobs.push({
      id: job.id,
      direction: 'upload',
      name: job.name,
      parentId: job.parentId,
      size: job.size,
      state,
      phase: null,
      bytesDone: 0,
      lanes: 0,
    });
    this.rows.set(job.id, this.jobs[this.jobs.length - 1] as JobProgress);
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
    job.resuming = undefined;
    (job.small ? this.queuedSmall : this.queuedLarge).push(job);
    this.pump();
  }

  /** Removes a finished row from the panel. */
  dismiss(id: string) {
    const row = this.rows.get(id);
    if (!row || ['queued', 'running', 'committing', 'stopped'].includes(row.state)) return;
    this.jobs = this.jobs.filter((r) => r.id !== id);
    this.rows.delete(id);
    this.internal.delete(id);
  }

  protected pump() {
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
        const start = {
          job: job.id,
          file: job.file as File,
          parentId: job.parentId,
          name: job.name,
          lanes: limits.lanes,
          clientMtime: job.lastModified,
        };
        const saved = job.resuming;
        this.send(
          saved
            ? { type: 'resume', ...start, ...savedSession(saved) }
            : { type: 'upload', ...start, clientKey: job.clientKey },
        );
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

  protected send(message: ToWorker) {
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

  protected receive(m: FromWorker) {
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
      case 'session':
      case 'resumed':
      case 'downloadProgress':
      case 'downloadDone':
      case 'downloadFailed':
        break;
    }
    this.pump();
  }

  protected batchDone(entries: BatchEntryResult[]) {
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

  protected batchFailed(commitId: string, failure: Failure) {
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

  protected fail(job: Job, failure: Failure, report = true) {
    if (report && failure.kind === 'api') this.deps.api.report(failure.error);
    this.failWith(job, uploadErrorOf(failure));
  }

  protected failWith(job: Job, error: UploadError) {
    const row = this.rows.get(job.id);
    if (!row) return;
    row.state = 'failed';
    row.phase = null;
    row.lanes = 0;
    row.error = error;
    row.errorText = UPLOAD_ERROR_TEXT[error];
  }

  protected failAllActive() {
    for (const row of this.jobs) {
      if (
        row.direction === 'upload' &&
        (row.state === 'running' || row.state === 'committing' || row.state === 'queued')
      ) {
        row.state = 'failed';
        row.error = 'Failed';
        row.errorText = UPLOAD_ERROR_TEXT.Failed;
      }
    }
  }

  protected finish(job: Job, row: JobProgress, state: 'done' | 'cancelled') {
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
