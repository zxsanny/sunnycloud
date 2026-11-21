import {
  CHANGED_SINCE_LAST_TIME_TEXT,
  STOPPED_TEXT,
  UPLOAD_ERROR_TEXT,
  uploadErrorOf,
} from './errors';
import { refuse, remove, type Job, type ResumeResult } from './jobs';
import type { FromWorker } from './protocol';
import { DownloadQueue } from './downloads.svelte';
import { matchesRecord } from './resume';

/**
 * The one facade pages use (contract transfer_service.md): the upload queue, downloads, and the resume of a large
 * upload that stopped because its tab closed or its file changed.
 */
export class TransferService extends DownloadQueue {
  // Plain map: no template reads it.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  private readonly resumeAnswers = new Map<string, (result: ResumeResult) => void>();

  /**
   * Continues a stopped upload with the file the person selected. The same file (name, size and modified time of the
   * record) sends only the blocks the server lacks; any difference ends the old session and uploads from the start.
   * Resolves once the session was read, not when the upload ends.
   */
  async resumeUpload(file: File, id: string): Promise<ResumeResult> {
    const job = this.internal.get(id);
    const row = this.rows.get(id);
    const record = job?.record;
    if (!job || !row || row.state !== 'stopped' || !record) {
      return refuse('Failed', UPLOAD_ERROR_TEXT.Failed);
    }
    const limits = await this.loadLimits();
    if ('ok' in limits) return limits;
    row.error = undefined;
    row.errorText = undefined;
    row.state = 'queued';
    if (!matchesRecord(file, record)) {
      await this.discard(job);
      Object.assign(job, {
        file,
        name: file.name,
        size: file.size,
        lastModified: file.lastModified,
        small: file.size <= limits.smallFileMax,
        clientKey: undefined,
        resuming: undefined,
      });
      Object.assign(row, {
        name: file.name,
        size: file.size,
        bytesDone: 0,
        note: CHANGED_SINCE_LAST_TIME_TEXT,
      });
      (job.small ? this.queuedSmall : this.queuedLarge).push(job);
      this.pump();
      return { ok: true, value: { kind: 'Restarted', reason: 'ChangedSinceLastTime' } };
    }
    job.file = file;
    job.resuming = record;
    row.note = undefined;
    return new Promise((resolve) => {
      this.resumeAnswers.set(id, resolve);
      this.queuedLarge.push(job);
      this.pump();
    });
  }

  /** Lists this account's resume records as stopped rows; the Files page calls it once the account is known. */
  async loadStopped(): Promise<void> {
    const accountId = this.deps.accountId?.() ?? null;
    if (this.deps.resume === undefined || accountId === null) return;
    const records = await this.deps.resume.list(accountId);
    // Rows of another account that signed in earlier in this tab are not this account's to see.
    this.jobs = this.jobs.filter((row) => {
      const foreign = row.state === 'stopped' && this.internal.get(row.id)?.accountId !== accountId;
      if (foreign) {
        this.rows.delete(row.id);
        this.internal.delete(row.id);
      }
      return !foreign;
    });
    const held = [...this.internal.values()].map((job) => job.clientKey);
    for (const record of records) {
      if (held.includes(record.clientKey)) continue;
      const job: Job = {
        id: `up-${(this.counter += 1)}`,
        file: null,
        parentId: record.parentId,
        name: record.fileName,
        size: record.size,
        lastModified: record.lastModified,
        small: false,
        clientKey: record.clientKey,
        accountId,
        record,
      };
      this.addRow(job, 'stopped');
      this.stop(job, STOPPED_TEXT.PageHidden);
    }
  }

  /**
   * The page is going away or is frozen and its Worker will not survive: every unfinished job ends. A large upload
   * with a session stops and keeps its resume record; the rest fail and can be retried.
   */
  suspend() {
    this.failDownloads();
    this.dispose();
    this.staging = 0;
    this.largeActive = 0;
    this.batch = null;
    this.capHold = false;
    this.pending.length = 0;
    this.queuedSmall.length = 0;
    this.queuedLarge.length = 0;
    for (const row of this.jobs) {
      const job = this.internal.get(row.id);
      if (!job || !['queued', 'running', 'committing'].includes(row.state)) continue;
      if (job.record) this.stop(job, STOPPED_TEXT.PageHidden);
      else this.failWith(job, 'Network');
      this.resumeAnswer(job.id, refuse('Network', UPLOAD_ERROR_TEXT.Network));
    }
  }

  /**
   * An upload with a resume record that will not continue (stopped, or re-selected and still queued) ends its server
   * session too, and a pending `resumeUpload` call is answered.
   */
  override cancelUpload(id: string) {
    const job = this.internal.get(id);
    const row = this.rows.get(id);
    if (job?.record && row && (row.state === 'stopped' || row.state === 'queued')) {
      remove(this.queuedLarge, job);
      void this.discard(job);
      this.resumeAnswer(id, refuse('Failed', UPLOAD_ERROR_TEXT.Failed));
      this.finish(job, row, 'cancelled');
      return;
    }
    super.cancelUpload(id);
  }

  /** A failed large upload that is dismissed will not be retried: its record and session go. */
  override dismiss(id: string) {
    const job = this.internal.get(id);
    if (job?.record && this.rows.get(id)?.state === 'failed') void this.discard(job);
    super.dismiss(id);
  }

  protected override receive(m: FromWorker) {
    const job = 'job' in m ? this.internal.get(m.job) : undefined;
    switch (m.type) {
      case 'session':
        if (!job) return;
        job.clientKey = m.clientKey;
        job.record = {
          fileName: job.name,
          size: job.size,
          lastModified: job.lastModified,
          sessionId: m.sessionId,
          clientKey: m.clientKey,
          blocklist: m.blocklist,
          parentId: job.parentId,
        };
        if (job.accountId !== null) void this.deps.resume?.put(job.accountId, job.record);
        return;
      case 'resumed':
        this.resumeAnswer(m.job, {
          ok: true,
          value: { kind: 'Resumed', resentBytes: m.resentBytes },
        });
        return;
      case 'done':
        if (job) {
          this.forget(job);
          this.resumeAnswer(m.job, { ok: true, value: { kind: 'Resumed', resentBytes: 0 } });
        }
        break;
      case 'cancelled':
        if (job && !job.small) {
          this.forget(job);
          this.resumeAnswer(m.job, refuse('Failed', UPLOAD_ERROR_TEXT.Failed));
        }
        break;
      case 'failed':
        super.receive(m);
        if (job) this.largeFailed(job, m);
        return;
    }
    super.receive(m);
  }

  /**
   * After the queue marked a large upload failed: a file that cannot be read or has changed keeps its record and the
   * row stops; a session the server no longer has loses its record, so Retry uploads the file again from the start.
   */
  private largeFailed(job: Job, m: Extract<FromWorker, { type: 'failed' }>) {
    const error = uploadErrorOf(m.failure);
    this.resumeAnswer(job.id, refuse(error, UPLOAD_ERROR_TEXT[error]));
    if ((error === 'FileUnreadable' || error === 'FileChanged') && job.record) {
      this.stop(job, STOPPED_TEXT[error]);
    } else if (error === 'UploadSessionGone') {
      this.forget(job);
      job.clientKey = undefined;
    }
  }

  /** The row waits for the person to select the file again; the record stays. */
  private stop(job: Job, text: string) {
    const row = this.rows.get(job.id);
    if (!row) return;
    row.state = 'stopped';
    row.phase = null;
    row.lanes = 0;
    row.error = undefined;
    row.errorText = text;
    job.resuming = undefined;
    job.file = null;
  }

  private resumeAnswer(id: string, result: ResumeResult) {
    this.resumeAnswers.get(id)?.(result);
    this.resumeAnswers.delete(id);
  }

  /** Removes the resume record: the upload committed, was cancelled, or its session is gone. */
  private forget(job: Job) {
    const record = job.record;
    job.record = undefined;
    if (record && job.accountId !== null)
      void this.deps.resume?.remove(job.accountId, record.clientKey);
  }

  /** Ends the server session of a stopped upload that will not continue (best effort: the server expires it). */
  private async discard(job: Job) {
    const record = job.record;
    if (!record) return;
    this.forget(job);
    const result = await this.deps.api.deleteUpload(record.sessionId);
    if (!result.ok) {
      console.warn(
        'The upload session could not be aborted; the server will expire it.',
        result.error.code,
      );
    }
  }
}
