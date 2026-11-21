import type { Api } from '$lib/api';
import type { UploadError } from './errors';
import type { FromWorker, ToWorker } from './protocol';
import type { ResumePort, ResumeRecord } from './resume';

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
  /** `stopped`: a large upload that ended with the tab (or its file) and keeps its resume record. */
  state: 'queued' | 'running' | 'committing' | 'done' | 'failed' | 'stopped' | 'cancelled';
  /** `preparing` is the hash pass of a large file, before any byte is sent. */
  phase: 'preparing' | 'sending' | null;
  bytesDone: number;
  lanes: number;
  error?: UploadError;
  errorText?: string;
  /** Something the person should know about a running job, such as a restart after the file changed. */
  note?: string;
}

export type Refusal = { ok: false; error: { code: UploadError; text: string } };
export type EnqueueResult = { ok: true; value: string[] } | Refusal;
export type ResumeOutcome =
  { kind: 'Resumed'; resentBytes: number } | { kind: 'Restarted'; reason: 'ChangedSinceLastTime' };
export type ResumeResult = { ok: true; value: ResumeOutcome } | Refusal;

export interface Job {
  id: string;
  file: File | null;
  parentId: string;
  name: string;
  size: number;
  lastModified: number;
  small: boolean;
  stagedKey?: string;
  clientKey?: string;
  /** The account that owns the job and its resume record. */
  accountId: string | null;
  /** Kept while a session exists, so a stopped job can be matched with the file the person selects. */
  record?: ResumeRecord;
  /** Set while a selected file continues `record`'s session instead of starting a new one. */
  resuming?: ResumeRecord;
}

export interface BatchInFlight {
  id: string;
  jobs: Job[];
  commitId?: string;
  attempts: number;
}

/** `web.max_selection_files`: a selection above it is refused before anything is queued. */
export const MAX_SELECTION_FILES = 100_000;
/** Small files staged at once. */
export const STAGE_PARALLEL = 6;
/** Large files in flight at once; each already uses all its lanes. */
export const LARGE_PARALLEL = 1;
export const BATCH_ATTEMPTS = 3;
export const BATCH_RETRY_MS = 1000;

export interface TransferDeps {
  api: Api;
  connect: () => WorkerLike;
  /** Delay unit between attempts to send a batch again; tests set 0. */
  batchRetryMs?: number;
  /** Resume records. Without it no upload can be resumed after the tab closed. */
  resume?: ResumePort;
  /** The signed-in account, once known: the owner of every resume record. */
  accountId?: () => string | null;
}

export function refuse(code: UploadError, text: string): Refusal {
  return { ok: false, error: { code, text } };
}

export function remove<T>(list: T[], item: T) {
  const at = list.indexOf(item);
  if (at !== -1) list.splice(at, 1);
}
