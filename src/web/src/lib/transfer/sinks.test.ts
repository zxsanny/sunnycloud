import { describe, expect, it } from 'vitest';
import { STALE_AFTER_MS, removeStale, type DirectoryLike } from './sinks';

const NOW = 1_800_000_000_000;

function directory(files: Record<string, number>) {
  const removed: string[] = [];
  const handle: DirectoryLike = {
    async *entries() {
      for (const [name, lastModified] of Object.entries(files)) {
        yield [name, { kind: 'file', getFile: async () => ({ lastModified }) }];
      }
    },
    async removeEntry(name) {
      removed.push(name);
    },
  };
  return { handle, removed };
}

describe('private download files left in the browser (OPFS)', () => {
  it('removes the ones older than an hour and keeps recent ones and files that are not downloads', async () => {
    // Arrange
    const { handle, removed } = directory({
      'dl-dn-1': NOW - STALE_AFTER_MS - 1,
      'dl-dn-2': NOW - STALE_AFTER_MS + 60_000,
      'other.bin': NOW - 10 * STALE_AFTER_MS,
    });

    // Act
    await removeStale(handle, NOW);

    // Assert
    expect(removed).toEqual(['dl-dn-1']);
  });
});
