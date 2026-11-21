import type { Api } from '$lib/api';
import {
  Cancelled,
  Mailbox,
  Stopped,
  apiFailure,
  hashesOf,
  defaultSleep,
  hex,
  readSlice,
  resentBytes,
  sendBlock,
  stampOf,
  type FileStamp,
  type Sent,
} from './blocks';
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

type UploadMessage = Extract<ToWorker, { type: 'upload' | 'resume' }>;
type SessionEnd = { kind: 'done'; nodeId: string | null; revisionId: string } | { kind: 'restart' };
/** An open session: the hash of every block, the blocks it still lacks, and whether the hashes came from a record. */
interface OpenSession {
  hashes: Uint8Array;
  blocklist: string[];
  uploadId: string;
  need: number[];
  fromRecord: boolean;
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
        case 'resume':
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
      } else if (message.type === 'upload' || message.type === 'resume') {
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
      const body = await readSlice(m.file, 0, m.file.size);
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

  private async upload(m: UploadMessage) {
    const controller = this.begin(m.job);
    const clientKey = m.type === 'resume' ? m.clientKey : (m.clientKey ?? this.newId());
    const progress = { sessionId: m.type === 'resume' ? m.sessionId : (null as string | null) };
    const stamp = stampOf(m.file);
    try {
      for (let attempt = 0; attempt <= MAX_RESTARTS; attempt += 1) {
        // Only the first attempt of a resume trusts the record; a restart hashes the file again.
        const record = attempt === 0 && m.type === 'resume' ? m : null;
        const end = await this.session(m, record, clientKey, controller.signal, progress, stamp);
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

  /** One session: open it (from the record or after a hash pass), send blocks as the planner says, commit. */
  private async session(
    m: UploadMessage,
    record: Extract<ToWorker, { type: 'resume' }> | null,
    clientKey: string,
    signal: AbortSignal,
    progress: { sessionId: string | null },
    stamp: FileStamp,
  ): Promise<SessionEnd> {
    const { wasm } = this.deps;
    const blockSize = wasm.block_size();
    const size = m.file.size;
    const count = Math.ceil(size / blockSize);
    const opened =
      (record && (await this.openFromRecord(record, count, signal))) ||
      (await this.openFresh(m, clientKey, count, blockSize, signal, progress));
    if ('kind' in opened) return opened;
    progress.sessionId = opened.uploadId;
    if (m.type === 'resume' && record) {
      const resent = resentBytes(opened.need, count, size, blockSize, m.lanes);
      this.deps.post({ type: 'resumed', job: m.job, resentBytes: resent });
    }
    return this.send(m, opened, count, blockSize, signal, stamp);
  }

  /**
   * The session of a resume record: its blocklist replaces the hash pass. Null when the record's blocklist cannot
   * describe this file (the caller then hashes it again); a session the server no longer has stops the job.
   */
  private async openFromRecord(
    m: Extract<ToWorker, { type: 'resume' }>,
    count: number,
    signal: AbortSignal,
  ): Promise<OpenSession | null> {
    const hashes = hashesOf(m.blocklist, count);
    if (hashes === null) {
      console.warn('The resume record does not fit the file; hashing the file again.');
      return null;
    }
    // A file that cannot be read stops the job before the server is asked anything.
    await readSlice(m.file, 0, Math.min(1, m.file.size));
    const state = await this.deps.api.getUpload(m.sessionId, signal);
    if (!state.ok) {
      if (signal.aborted) throw new Cancelled();
      if (state.error.code === 'upload_session_gone') {
        throw new Stopped({ kind: 'local', reason: 'SessionGone' });
      }
      throw new Stopped(apiFailure(state.error));
    }
    return {
      hashes,
      blocklist: m.blocklist,
      uploadId: m.sessionId,
      need: state.value.need_blocks,
      fromRecord: true,
    };
  }

  /** The hash of every block (only the 32-byte hashes are kept), then `POST /uploads`. */
  private async openFresh(
    m: UploadMessage,
    clientKey: string,
    count: number,
    blockSize: number,
    signal: AbortSignal,
    progress: { sessionId: string | null },
  ): Promise<OpenSession | SessionEnd> {
    const { wasm, api } = this.deps;
    const size = m.file.size;
    const hashes = new Uint8Array(count * 32);
    let lastPost = 0;
    for (let i = 0; i < count; i += 1) {
      if (signal.aborted) throw new Cancelled();
      const end = Math.min(size, (i + 1) * blockSize);
      hashes.set(wasm.hash_block(await readSlice(m.file, i * blockSize, end)), i * 32);
      lastPost = this.postProgress(m.job, 'preparing', end, 0, lastPost);
    }
    const blocklist = Array.from({ length: count }, (_, i) =>
      hex(hashes.subarray(i * 32, (i + 1) * 32)),
    );
    const started = await api.postUploads(
      {
        parent_id: m.parentId,
        name: m.name,
        declared_size: size,
        client_key: clientKey,
        content_hash: hex(wasm.rollup(hashes)),
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
    progress.sessionId = started.value.upload_id;
    this.deps.post({
      type: 'session',
      job: m.job,
      clientKey,
      sessionId: started.value.upload_id,
      blocklist,
    });
    return {
      hashes,
      blocklist,
      uploadId: started.value.upload_id,
      need: started.value.need_blocks ?? [],
      fromRecord: false,
    };
  }

  /** Before commit the file is looked at again: an edit since the upload began stops the job, nothing is committed. */
  private async assertUnchanged(file: Blob, stamp: FileStamp) {
    const now = stampOf(file);
    if (now.size !== stamp.size || now.lastModified !== stamp.lastModified) {
      throw new Stopped({ kind: 'local', reason: 'FileChanged' });
    }
    try {
      // A browser that keeps a File as a snapshot cannot show a changed size or time, but refuses to read it.
      await readSlice(file, 0, Math.min(1, file.size));
    } catch (cause) {
      if (cause instanceof Stopped) throw new Stopped({ kind: 'local', reason: 'FileChanged' });
      throw cause;
    }
  }

  /** The planner loop of one open session. */
  private async send(
    m: UploadMessage,
    opened: OpenSession,
    count: number,
    blockSize: number,
    signal: AbortSignal,
    stamp: FileStamp,
  ): Promise<SessionEnd> {
    const { wasm, api } = this.deps;
    const { hashes, blocklist, uploadId, need } = opened;
    const size = m.file.size;
    const blockEnd = (i: number) => Math.min(size, (i + 1) * blockSize);
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
    let lastPost = this.postProgress(m.job, 'sending', bytesDone, 0, 0);

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
              void sendBlock(
                api,
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
              await this.assertUnchanged(m.file, stamp);
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
          // A hash the server refuses, from a record, means the record is not to be trusted: hash the file again.
          if (opened.fromRecord && sent.outcome.kind === 'HashRejected') return { kind: 'restart' };
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
}
