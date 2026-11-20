import { isTransientError, type Api, type ApiError } from '$lib/api';
import type { BatchEntryResult, Failure, FromWorker, ToWorker } from './protocol';
import type { WasmApi } from './wasm';

export interface EngineDeps {
  wasm: WasmApi;
  api: Api;
  post: (message: FromWorker) => void;
  now?: () => number;
  random?: (bytes: number) => Uint8Array;
  sleep?: (ms: number, signal: AbortSignal) => Promise<void>;
}

/** Retry timing of one block (`sync.retry_base`, `sync.retry_max`, `sync.max_attempts_per_block`). */
const RETRY_BASE_MS = 500;
const RETRY_MAX_MS = 10_000;
const MAX_ATTEMPTS_PER_BLOCK = 8;
/** A new session after a mismatch or a gone session, with a fresh hash pass. */
const MAX_RESTARTS = 2;
const PROGRESS_EVERY_MS = 100;

const hex = (bytes: Uint8Array) =>
  Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');

class Cancelled extends Error {}
class Stopped extends Error {
  constructor(readonly failure: Failure) {
    super('transfer stopped');
  }
}

function defaultSleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(done, ms);
    signal.addEventListener('abort', done, { once: true });
    function done() {
      clearTimeout(timer);
      signal.removeEventListener('abort', done);
      resolve();
    }
  });
}

/** Results of block sends, in the order they finish. */
class Mailbox<T> {
  private items: T[] = [];
  private waiting: ((item: T) => void) | null = null;

  push(item: T) {
    if (this.waiting) {
      const wake = this.waiting;
      this.waiting = null;
      wake(item);
    } else {
      this.items.push(item);
    }
  }

  take(): Promise<T> {
    const item = this.items.shift();
    if (item !== undefined) return Promise.resolve(item);
    return new Promise((resolve) => {
      this.waiting = resolve;
    });
  }
}

type BlockOutcome =
  | { kind: 'Acked' | 'HashRejected' | 'SessionGone' }
  | { kind: 'Transient' | 'BlockConflict'; retry_after_ms?: number };
type Sent = { i: number; outcome: BlockOutcome } | { stop: Failure } | { cancelled: true };

type SessionEnd = { kind: 'done'; nodeId: string | null; revisionId: string } | { kind: 'restart' };

function apiFailure(error: ApiError): Failure {
  return { kind: 'api', error };
}

/**
 * The Worker's logic, apart from the Worker shell so Node can run it: hashing and every planning rule come from
 * sync-core-wasm, every request goes through the API client. A file is read one block at a time, hashed, sent and
 * dropped: memory stays at a few blocks whatever the file size.
 */
export class UploadEngine {
  private readonly jobs = new Map<string, AbortController>();
  private readonly now: () => number;
  private readonly random: (bytes: number) => Uint8Array;
  private readonly sleep: (ms: number, signal: AbortSignal) => Promise<void>;

  constructor(private readonly deps: EngineDeps) {
    this.now = deps.now ?? Date.now;
    this.random = deps.random ?? ((n) => crypto.getRandomValues(new Uint8Array(n)));
    this.sleep = deps.sleep ?? defaultSleep;
  }

  /** Never rejects: an unexpected error is reported to the page as an internal failure of that job. */
  async handle(message: ToWorker): Promise<void> {
    try {
      switch (message.type) {
        case 'stage':
          return await this.stage(message);
        case 'batch':
          return await this.batch(message);
        case 'upload':
          return await this.upload(message);
        case 'cancel':
          this.jobs.get(message.job)?.abort();
          return;
      }
    } catch (cause) {
      console.error('The transfer engine hit an unexpected error.', cause);
      const failure: Failure = { kind: 'local', reason: 'Internal' };
      if (message.type === 'stage') {
        this.deps.post({ type: 'stageFailed', job: message.job, stagedKey: '', failure });
      } else if (message.type === 'upload') {
        this.deps.post({ type: 'failed', job: message.job, clientKey: null, failure });
      } else if (message.type === 'batch') {
        this.deps.post({
          type: 'batchFailed',
          batch: message.batch,
          commitId: message.commitId ?? '',
          failure,
        });
      }
    }
  }

  private newId(): string {
    return this.deps.wasm.new_commit_id(this.now(), this.random(10));
  }

  private begin(job: string): AbortController {
    const controller = new AbortController();
    this.jobs.set(job, controller);
    return controller;
  }

  private async stage(m: Extract<ToWorker, { type: 'stage' }>) {
    const controller = this.begin(m.job);
    const stagedKey = m.stagedKey ?? this.newId();
    try {
      const body = await this.read(m.file, 0, m.file.size);
      const hash = hex(this.deps.wasm.hash_block(body));
      const result = await this.deps.api.putStaged(stagedKey, body, hash, controller.signal);
      if (result.ok) {
        this.deps.post({ type: 'staged', job: m.job, stagedKey });
      } else if (controller.signal.aborted) {
        this.deps.post({ type: 'cancelled', job: m.job });
      } else {
        this.deps.post({
          type: 'stageFailed',
          job: m.job,
          stagedKey,
          failure: apiFailure(result.error),
        });
      }
    } catch (cause) {
      if (!(cause instanceof Stopped)) throw cause;
      this.deps.post({ type: 'stageFailed', job: m.job, stagedKey, failure: cause.failure });
    } finally {
      this.jobs.delete(m.job);
    }
  }

  private async batch(m: Extract<ToWorker, { type: 'batch' }>) {
    const commitId = m.commitId ?? this.newId();
    const result = await this.deps.api.postCommitsBatch({
      commit_id: commitId,
      entries: m.entries.map((entry) => ({
        staged_key: entry.stagedKey,
        parent_id: entry.parentId,
        name: entry.name,
        client_mtime:
          entry.clientMtime === undefined ? undefined : new Date(entry.clientMtime).toISOString(),
      })),
    });
    if (!result.ok) {
      this.deps.post({
        type: 'batchFailed',
        batch: m.batch,
        commitId,
        failure: apiFailure(result.error),
      });
      return;
    }
    const entries: BatchEntryResult[] = result.value.entries.map((entry) =>
      entry.status === 'committed' && entry.node_id && entry.revision_id
        ? { status: 'committed', nodeId: entry.node_id, revisionId: entry.revision_id }
        : { status: 'error', code: entry.code ?? 'internal' },
    );
    this.deps.post({ type: 'batchDone', batch: m.batch, commitId, entries });
  }

  /** Reads `[start, end)` of the file; a read that fails (file moved or deleted) stops that job. */
  private async read(file: Blob, start: number, end: number): Promise<Uint8Array> {
    try {
      return new Uint8Array(await file.slice(start, end).arrayBuffer());
    } catch (cause) {
      if (cause instanceof DOMException) {
        throw new Stopped({ kind: 'local', reason: 'FileUnreadable' });
      }
      throw cause;
    }
  }

  private async upload(m: Extract<ToWorker, { type: 'upload' }>) {
    const controller = this.begin(m.job);
    const clientKey = m.clientKey ?? this.newId();
    const progress = { sessionId: null as string | null };
    try {
      for (let attempt = 0; attempt <= MAX_RESTARTS; attempt += 1) {
        const end = await this.session(m, clientKey, controller.signal, progress);
        if (end.kind === 'done') {
          this.deps.post({
            type: 'done',
            job: m.job,
            clientKey,
            nodeId: end.nodeId,
            revisionId: end.revisionId,
          });
          return;
        }
      }
      throw new Stopped({ kind: 'local', reason: 'RestartLimit' });
    } catch (cause) {
      if (cause instanceof Cancelled) {
        await this.abortSession(progress.sessionId);
        this.deps.post({ type: 'cancelled', job: m.job });
      } else if (cause instanceof Stopped) {
        this.deps.post({ type: 'failed', job: m.job, clientKey, failure: cause.failure });
      } else {
        throw cause;
      }
    } finally {
      this.jobs.delete(m.job);
    }
  }

  private async abortSession(sessionId: string | null) {
    if (sessionId === null) return;
    const result = await this.deps.api.deleteUpload(sessionId);
    if (!result.ok) {
      console.warn(
        'The upload session could not be aborted; the server will expire it.',
        result.error.code,
      );
    }
  }

  /** One session: hash pass, start, send blocks as the planner says, commit. */
  private async session(
    m: Extract<ToWorker, { type: 'upload' }>,
    clientKey: string,
    signal: AbortSignal,
    progress: { sessionId: string | null },
  ): Promise<SessionEnd> {
    const { wasm, api } = this.deps;
    const blockSize = wasm.block_size();
    const size = m.file.size;
    const count = Math.ceil(size / blockSize);
    const blockEnd = (i: number) => Math.min(size, (i + 1) * blockSize);

    // Pass 1: the hash of every block. Only the 32-byte hashes are kept.
    const hashes = new Uint8Array(count * 32);
    let lastPost = 0;
    for (let i = 0; i < count; i += 1) {
      if (signal.aborted) throw new Cancelled();
      hashes.set(wasm.hash_block(await this.read(m.file, i * blockSize, blockEnd(i))), i * 32);
      lastPost = this.postProgress(m.job, 'preparing', blockEnd(i), 0, lastPost);
    }
    const blocklist = Array.from({ length: count }, (_, i) =>
      hex(hashes.subarray(i * 32, (i + 1) * 32)),
    );
    const fileHash = hex(wasm.rollup(hashes));

    const started = await api.postUploads(
      {
        parent_id: m.parentId,
        name: m.name,
        declared_size: size,
        client_key: clientKey,
        content_hash: fileHash,
      },
      signal,
    );
    if (!started.ok) {
      if (signal.aborted) throw new Cancelled();
      throw new Stopped(apiFailure(started.error));
    }
    if (started.value.outcome === 'nothing_to_send' || !started.value.upload_id) {
      return { kind: 'done', nodeId: null, revisionId: started.value.revision_id ?? '' };
    }
    const uploadId = started.value.upload_id;
    progress.sessionId = uploadId;
    const need = started.value.need_blocks ?? [];

    const planner = new wasm.UploadPlanner(
      count,
      Uint32Array.from(need),
      m.lanes,
      RETRY_BASE_MS,
      RETRY_MAX_MS,
      MAX_ATTEMPTS_PER_BLOCK,
    );
    const mailbox = new Mailbox<Sent>();
    // A fatal problem stops the lanes that are still running.
    const lanes = new AbortController();
    const stopLanes = () => lanes.abort();
    signal.addEventListener('abort', stopLanes, { once: true });

    let bytesDone = size - need.reduce((sum, i) => sum + (blockEnd(i) - i * blockSize), 0);
    let inFlight = 0;
    let committed: { nodeId: string; revisionId: string } | null = null;
    lastPost = this.postProgress(m.job, 'sending', bytesDone, 0, 0);

    try {
      for (;;) {
        const actions = planner.next_actions().map((a) => {
          const action = { kind: a.kind, i: a.i, ms: a.ms };
          a.free();
          return action;
        });
        for (const action of actions) {
          if (signal.aborted) throw new Cancelled();
          switch (action.kind) {
            case 'SendBlock': {
              const i = action.i ?? 0;
              inFlight += 1;
              void this.sendBlock(
                m.file,
                i,
                blockEnd(i),
                blockSize,
                uploadId,
                hashes,
                lanes.signal,
              ).then((sent) => mailbox.push(sent));
              break;
            }
            case 'Wait':
              await this.sleep(action.ms ?? 0, signal);
              break;
            case 'RereadSession': {
              const state = await api.getUpload(uploadId, signal);
              if (!state.ok) {
                if (signal.aborted) throw new Cancelled();
                if (state.error.code === 'upload_session_gone') return { kind: 'restart' };
                throw new Stopped(apiFailure(state.error));
              }
              planner.on_session(Uint32Array.from(state.value.need_blocks));
              break;
            }
            case 'Commit': {
              const result = await api.postCommits(
                {
                  upload_id: uploadId,
                  parent_id: m.parentId,
                  name: m.name,
                  blocklist,
                  client_mtime:
                    m.clientMtime === undefined ? undefined : new Date(m.clientMtime).toISOString(),
                },
                signal,
              );
              if (result.ok) {
                committed = { nodeId: result.value.node_id, revisionId: result.value.revision_id };
                planner.on_commit({ kind: 'Ok' });
              } else if (signal.aborted) {
                throw new Cancelled();
              } else if (result.error.code === 'need_blocks') {
                planner.on_commit({ kind: 'NeedBlocks', list: result.error.needBlocks ?? [] });
              } else if (result.error.code === 'blocklist_mismatch') {
                planner.on_commit({ kind: 'BlocklistMismatch' });
              } else if (result.error.code === 'upload_session_gone') {
                planner.on_commit({ kind: 'SessionGone' });
              } else {
                throw new Stopped(apiFailure(result.error));
              }
              break;
            }
            case 'Done':
              if (committed === null) throw new Stopped({ kind: 'local', reason: 'Internal' });
              return { kind: 'done', ...committed };
            case 'RestartSession':
              return { kind: 'restart' };
            case 'GiveUp':
              throw new Stopped({ kind: 'local', reason: 'BlockRetriesExhausted' });
          }
        }
        if (inFlight > 0) {
          const sent = await mailbox.take();
          inFlight -= 1;
          if ('cancelled' in sent) throw new Cancelled();
          if ('stop' in sent) throw new Stopped(sent.stop);
          if (sent.outcome.kind === 'Acked') bytesDone += blockEnd(sent.i) - sent.i * blockSize;
          planner.on_result(sent.i, sent.outcome);
          lastPost = this.postProgress(m.job, 'sending', bytesDone, inFlight, lastPost);
        } else if (actions.length === 0) {
          throw new Stopped({ kind: 'local', reason: 'Internal' });
        }
      }
    } finally {
      signal.removeEventListener('abort', stopLanes);
      lanes.abort();
      planner.free();
    }
  }

  private postProgress(
    job: string,
    phase: 'preparing' | 'sending',
    bytesDone: number,
    lanes: number,
    lastPost: number,
  ): number {
    const at = this.now();
    if (at - lastPost < PROGRESS_EVERY_MS) return lastPost;
    this.deps.post({ type: 'progress', job, phase, bytesDone, lanes });
    return at;
  }

  /** One block, one request, no streaming body. Never throws: the outcome goes to the mailbox. */
  private async sendBlock(
    file: Blob,
    i: number,
    end: number,
    blockSize: number,
    uploadId: string,
    hashes: Uint8Array,
    signal: AbortSignal,
  ): Promise<Sent> {
    let bytes: Uint8Array;
    try {
      bytes = new Uint8Array(await file.slice(i * blockSize, end).arrayBuffer());
    } catch (cause) {
      if (cause instanceof DOMException)
        return { stop: { kind: 'local', reason: 'FileUnreadable' } };
      throw cause;
    }
    const result = await this.deps.api.putBlock(
      uploadId,
      i,
      bytes,
      hex(hashes.subarray(i * 32, (i + 1) * 32)),
      signal,
    );
    if (result.ok) return { i, outcome: { kind: 'Acked' } };
    if (signal.aborted) return { cancelled: true };
    const error = result.error;
    const retryAfterMs =
      error.retryAfterSeconds === null ? undefined : error.retryAfterSeconds * 1000;
    if (error.code === 'block_hash_mismatch' || error.code === 'block_length_mismatch') {
      return { i, outcome: { kind: 'HashRejected' } };
    }
    if (error.code === 'block_conflict') {
      return { i, outcome: { kind: 'BlockConflict', retry_after_ms: retryAfterMs } };
    }
    if (error.code === 'upload_session_gone') return { i, outcome: { kind: 'SessionGone' } };
    if (isTransientError(error))
      return { i, outcome: { kind: 'Transient', retry_after_ms: retryAfterMs } };
    return { stop: apiFailure(error) };
  }
}
