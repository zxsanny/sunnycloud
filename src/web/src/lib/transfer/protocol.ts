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

/** Why a download ended without a good file; the page maps it to the contract's `Failed` outcome. */
export type DownloadFailure =
  'HashMismatch' | 'Forbidden' | 'NotFound' | 'Network' | 'SaveFailed' | 'Cancelled';

/** Which content route a download reads (each answers Range); for a share link `id` is its token. */
export type ContentSource = { kind: 'node' | 'revision' | 'share'; id: string };

/** Where verified bytes go: a file the person chose in the save dialog, or a private browser file (OPFS) for later. */
export type DownloadSink =
  { kind: 'picker'; handle: FileSystemFileHandle } | { kind: 'opfs'; name: string };

export type ToWorker =
  /**
   * Download and verify: read `size` bytes in ranged pulls, hash each 2 MiB block as it arrives, and keep the bytes
   * only if the roll-up equals `hash`. Nothing is kept in memory but the block being read.
   */
  | {
      type: 'download';
      job: string;
      source: ContentSource;
      size: number;
      hash: string;
      sink: DownloadSink;
    }
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
  /**
   * The file of a stopped large upload, selected again: read the session, send only the blocks it lacks. The
   * blocklist comes from the resume record (it saves the hash pass); the server still checks every block.
   */
  | {
      type: 'resume';
      job: string;
      file: Blob;
      parentId: string;
      name: string;
      lanes: number;
      clientKey: string;
      sessionId: string;
      blocklist: string[];
      clientMtime?: number;
    }
  | { type: 'cancel'; job: string };

export type LocalReason =
  | 'FileUnreadable'
  | 'FileChanged'
  | 'SessionGone'
  | 'BlockRetriesExhausted'
  | 'RestartLimit'
  | 'Internal';

/** Why something failed: the server's answer, or a problem on this side. */
export type Failure = { kind: 'api'; error: ApiError } | { kind: 'local'; reason: LocalReason };

export type FromWorker =
  | { type: 'ready' }
  | { type: 'fatal'; message: string }
  | { type: 'staged'; job: string; stagedKey: string }
  | { type: 'stageFailed'; job: string; stagedKey: string; failure: Failure }
  | { type: 'batchDone'; batch: string; commitId: string; entries: BatchEntryResult[] }
  | { type: 'batchFailed'; batch: string; commitId: string; failure: Failure }
  /** A session exists for this job: the page keeps a resume record until the upload ends. */
  | { type: 'session'; job: string; clientKey: string; sessionId: string; blocklist: string[] }
  /** The session of a resumed job was read; `resentBytes` is the most it may send a second time (see `resentBytes`). */
  | { type: 'resumed'; job: string; resentBytes: number }
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
  | { type: 'cancelled'; job: string }
  | { type: 'downloadProgress'; job: string; bytesDone: number }
  /** The roll-up matched and the bytes are saved. `file` is set for an `opfs` sink: the page hands it to the browser. */
  | { type: 'downloadDone'; job: string; file?: File }
  | { type: 'downloadFailed'; job: string; reason: DownloadFailure };
