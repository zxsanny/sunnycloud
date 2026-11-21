/**
 * The resume record of a large upload (contract transfer_service.md): what is needed to continue a session after the
 * tab closed. It never holds a token, password or cookie value, and every record belongs to one account.
 */
export interface ResumeRecord {
  fileName: string;
  size: number;
  lastModified: number;
  sessionId: string;
  /** Random UUIDv7 created with the session, reused on every resume. */
  clientKey: string;
  /** The hash of every block, so a resume does not hash the file again. The server checks every block anyway. */
  blocklist: string[];
  /** The folder the file goes to; the session read of the server does not return it. */
  parentId: string;
}

/** The three members of a record that say whether a selected file is the one that was uploading. */
export const matchesRecord = (
  file: { name: string; size: number; lastModified: number },
  record: ResumeRecord,
): boolean =>
  file.name === record.fileName &&
  file.size === record.size &&
  file.lastModified === record.lastModified;

export interface ResumePort {
  list(accountId: string): Promise<ResumeRecord[]>;
  put(accountId: string, record: ResumeRecord): Promise<void>;
  remove(accountId: string, clientKey: string): Promise<void>;
}

const DATABASE = 'sunnycloud-transfer';
const STORE = 'resume';

/**
 * Records in IndexedDB, keyed by account and client key. No call rejects: when the browser refuses (no IndexedDB, a
 * blocked or full store) the failure is logged, `onUnavailable` runs once, and reads then answer with nothing.
 */
export class ResumeStore implements ResumePort {
  private database: Promise<IDBDatabase | null> | null = null;
  private notified = false;

  constructor(
    private readonly onUnavailable: () => void,
    /** Read when the first call opens the database: touching `indexedDB` can throw where storage is blocked. */
    private readonly factory: () => IDBFactory | undefined = () => globalThis.indexedDB,
  ) {}

  async list(accountId: string): Promise<ResumeRecord[]> {
    const rows = await this.run<Stored[]>('readonly', (store) =>
      store.getAll(IDBKeyRange.bound([accountId, ''], [accountId, '￿'])),
    );
    return (rows ?? []).map((row) => ({
      fileName: row.fileName,
      size: row.size,
      lastModified: row.lastModified,
      sessionId: row.sessionId,
      clientKey: row.clientKey,
      blocklist: row.blocklist,
      parentId: row.parentId,
    }));
  }

  async put(accountId: string, record: ResumeRecord): Promise<void> {
    // Field by field, so nothing but the contract's members can reach storage.
    const stored: Stored = {
      accountId,
      fileName: record.fileName,
      size: record.size,
      lastModified: record.lastModified,
      sessionId: record.sessionId,
      clientKey: record.clientKey,
      blocklist: record.blocklist,
      parentId: record.parentId,
    };
    await this.run('readwrite', (store) => store.put(stored));
  }

  async remove(accountId: string, clientKey: string): Promise<void> {
    await this.run('readwrite', (store) => store.delete([accountId, clientKey]));
  }

  private unavailable(cause: unknown) {
    console.warn('Resume records are unavailable; uploads continue without resume.', cause);
    if (this.notified) return;
    this.notified = true;
    this.onUnavailable();
  }

  private open(): Promise<IDBDatabase | null> {
    this.database ??= new Promise((resolve) => {
      let request: IDBOpenDBRequest;
      try {
        const factory = this.factory();
        if (factory === undefined) throw new Error('IndexedDB is not available in this browser.');
        request = factory.open(DATABASE, 1);
      } catch (cause) {
        this.unavailable(cause);
        resolve(null);
        return;
      }
      request.onupgradeneeded = () =>
        request.result.createObjectStore(STORE, { keyPath: ['accountId', 'clientKey'] });
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => {
        this.unavailable(request.error);
        resolve(null);
      };
      request.onblocked = () => {
        this.unavailable(new Error('The resume database is blocked by another tab.'));
        resolve(null);
      };
    });
    return this.database;
  }

  private async run<T>(
    mode: IDBTransactionMode,
    action: (store: IDBObjectStore) => IDBRequest<T>,
  ): Promise<T | null> {
    const database = await this.open();
    if (database === null) return null;
    return new Promise((resolve) => {
      try {
        const tx = database.transaction(STORE, mode);
        const request = action(tx.objectStore(STORE));
        // Done when the transaction commits, not when the request is queued.
        tx.oncomplete = () => resolve(request.result);
        tx.onabort = tx.onerror = () => {
          this.unavailable(tx.error);
          resolve(null);
        };
      } catch (cause) {
        this.unavailable(cause);
        resolve(null);
      }
    });
  }
}

type Stored = ResumeRecord & { accountId: string };

/** The part of a record the Worker needs to continue its session. */
export const savedSession = (record: ResumeRecord) => ({
  clientKey: record.clientKey,
  sessionId: record.sessionId,
  blocklist: record.blocklist,
});
