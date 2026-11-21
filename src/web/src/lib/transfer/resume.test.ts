import { IDBFactory, IDBKeyRange } from 'fake-indexeddb';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { type ResumeRecord, ResumeStore, matchesRecord } from './resume';

const record = (over: Partial<ResumeRecord> = {}): ResumeRecord => ({
  fileName: 'archive.zip',
  size: 5_000_000,
  lastModified: 1_700_000_000_000,
  sessionId: '0190aaaa-0000-7000-8000-000000000001',
  clientKey: '0190bbbb-0000-7000-8000-000000000001',
  blocklist: ['aa'.repeat(32), 'bb'.repeat(32)],
  parentId: '0190aaaa-0000-7000-8000-00000000f001',
  ...over,
});

const store = (onUnavailable = vi.fn(), factory = new IDBFactory()) => ({
  store: new ResumeStore(onUnavailable, () => factory),
  onUnavailable,
  factory,
});

// A browser has IDBKeyRange as a global; the fake gives it as an export.
beforeEach(() => vi.stubGlobal('IDBKeyRange', IDBKeyRange));
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe('matching a selected file with a resume record', () => {
  const file = (over: Partial<{ name: string; size: number; lastModified: number }> = {}) => ({
    name: 'archive.zip',
    size: 5_000_000,
    lastModified: 1_700_000_000_000,
    ...over,
  });

  it('matches when name, size and modified time are all equal', () => {
    // Arrange
    const selected = file();

    // Act
    const same = matchesRecord(selected, record());

    // Assert
    expect(same).toBe(true);
  });

  it.each([
    ['another modified time', { lastModified: 1_700_000_001_000 }],
    ['another size', { size: 5_000_001 }],
    ['another name', { name: 'archive-2.zip' }],
  ])('does not match with %s', (_label, change) => {
    // Arrange
    const selected = file(change);

    // Act
    const same = matchesRecord(selected, record());

    // Assert
    expect(same).toBe(false);
  });
});

describe('the resume records of IndexedDB', () => {
  it('keeps a record for its account only', async () => {
    // Arrange
    const { store: records } = store();
    await records.put('account-a', record());

    // Act
    const own = await records.list('account-a');
    const other = await records.list('account-b');

    // Assert
    expect(own).toEqual([record()]);
    expect(other).toEqual([]);
  });

  it('keeps the same client key of two accounts apart and removes only the named one', async () => {
    // Arrange
    const { store: records } = store();
    await records.put('account-a', record());
    await records.put('account-b', record());

    // Act
    await records.remove('account-a', record().clientKey);

    // Assert
    expect(await records.list('account-a')).toEqual([]);
    expect(await records.list('account-b')).toEqual([record()]);
  });

  it('stores nothing but the members of the contract, whatever the caller passes', async () => {
    // Arrange
    const { store: records, factory } = store();
    const withSecrets = {
      ...record(),
      token: 'secret-token',
      password: 'hunter2',
      cookie: '__Host-session=abc',
    } as ResumeRecord;

    // Act
    await records.put('account-a', withSecrets);

    // Assert
    const dump = await new Promise<unknown[]>((resolve, reject) => {
      const open = factory.open('sunnycloud-transfer', 1);
      open.onerror = () => reject(open.error);
      open.onsuccess = () => {
        const all = open.result.transaction('resume').objectStore('resume').getAll();
        all.onsuccess = () => resolve(all.result);
      };
    });
    expect(Object.keys(dump[0] as object).sort()).toEqual(
      [
        'accountId',
        'blocklist',
        'clientKey',
        'fileName',
        'lastModified',
        'parentId',
        'sessionId',
        'size',
      ].sort(),
    );
    expect(JSON.stringify(dump)).not.toMatch(/secret-token|hunter2|__Host-session/);
  });

  it('says once that records are unavailable when the browser has no IndexedDB', async () => {
    // Arrange
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    const onUnavailable = vi.fn();
    const records = new ResumeStore(onUnavailable, () => undefined);

    // Act
    await records.put('account-a', record());
    const listed = await records.list('account-a');
    await records.remove('account-a', record().clientKey);

    // Assert
    expect(listed).toEqual([]);
    expect(onUnavailable).toHaveBeenCalledTimes(1);
    expect(console.warn).toHaveBeenCalled();
  });

  it('says so when opening the database throws, as a browser with blocked storage does', async () => {
    // Arrange
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    const onUnavailable = vi.fn();
    const blocked = {
      open() {
        throw new DOMException('The operation is insecure.', 'SecurityError');
      },
    } as unknown as IDBFactory;
    const records = new ResumeStore(onUnavailable, () => blocked);

    // Act
    await records.put('account-a', record());

    // Assert
    expect(onUnavailable).toHaveBeenCalledTimes(1);
    expect(await records.list('account-a')).toEqual([]);
  });
});
