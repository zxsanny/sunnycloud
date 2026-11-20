import type { ApiError } from '$lib/api';

/** What the page hands to the transfer Worker, and what comes back. Everything here is structured-clone data. */

export interface BatchEntry {
  stagedKey: string;
  parentId: string;
  name: string;
  clientMtime?: number;
}

export type BatchEntryResult =
  { status: 'committed'; nodeId: string; revisionId: string } | { status: 'error'; code: string };

export type ToWorker =
  /** Small file (at most one block): hash, then `PUT /staged-uploads/{key}`. */
  | { type: 'stage'; job: string; file: Blob; stagedKey?: string }
  /** `POST /commits:batch`. The commit ID is made here (sync-core-wasm) and handed back so a retry reuses it. */
  | { type: 'batch'; batch: string; commitId?: string; entries: BatchEntry[] }
  /** Large file: hash every block, open a session, send blocks on up to `lanes` lanes, commit. */
  | {
      type: 'upload';
      job: string;
      file: Blob;
      parentId: string;
      name: string;
      lanes: number;
      clientKey?: string;
      clientMtime?: number;
    }
  | { type: 'cancel'; job: string };

export type LocalReason = 'FileUnreadable' | 'BlockRetriesExhausted' | 'RestartLimit' | 'Internal';

/** Why something failed: the server's answer, or a problem on this side. */
export type Failure = { kind: 'api'; error: ApiError } | { kind: 'local'; reason: LocalReason };

export type FromWorker =
  | { type: 'ready' }
  | { type: 'fatal'; message: string }
  | { type: 'staged'; job: string; stagedKey: string }
  | { type: 'stageFailed'; job: string; stagedKey: string; failure: Failure }
  | { type: 'batchDone'; batch: string; commitId: string; entries: BatchEntryResult[] }
  | { type: 'batchFailed'; batch: string; commitId: string; failure: Failure }
  | {
      type: 'progress';
      job: string;
      phase: 'preparing' | 'sending';
      bytesDone: number;
      lanes: number;
    }
  | {
      type: 'done';
      job: string;
      clientKey: string;
      /** Null when the server already held this exact content (`nothing_to_send`). */
      nodeId: string | null;
      revisionId: string;
    }
  | { type: 'failed'; job: string; clientKey: string | null; failure: Failure }
  | { type: 'cancelled'; job: string };
