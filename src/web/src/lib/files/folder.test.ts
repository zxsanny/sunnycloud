import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { TreeServer, makeNode } from '../../../tests/support/treeServer';
import { folderStore } from './folder.svelte';

let server: TreeServer;

beforeEach(async () => {
  server = new TreeServer();
  vi.stubGlobal('fetch', (request: Request) => server.fetch(request));
  server.add(server.root, { name: 'b-folder', kind: 'folder' });
  server.add(server.root, { name: 'b.txt', kind: 'file' });
  server.add(server.root, { name: 'd.txt', kind: 'file' });
  await folderStore.open(null);
});
afterEach(() => vi.unstubAllGlobals());

const names = () => folderStore.items.map((i) => i.name);
const row = (name: string, extra: object = {}) =>
  makeNode({ name, kind: 'file', parent_id: server.root.node_id, seq: 5, ...extra });

describe('the folder store: first load and paging', () => {
  it('opens the root, lists folders first and reads the ancestors by id', async () => {
    // Assert
    expect(folderStore.status).toBe('ready');
    expect(names()).toEqual(['b-folder', 'b.txt', 'd.txt']);
    await vi.waitFor(() =>
      expect(folderStore.trail.map((n) => n.node_id)).toEqual([server.root.node_id]),
    );
  });

  it('walks up the parents for the trail of a nested folder', async () => {
    // Arrange
    const inner = server.add(
      server.nodes.get(
        [...server.nodes.values()].find((n) => n.name === 'b-folder')?.node_id ?? '',
      )!,
      {
        name: 'inner',
        kind: 'folder',
      },
    );

    // Act
    await folderStore.open(inner.node_id);

    // Assert
    await vi.waitFor(() =>
      expect(folderStore.trail.map((n) => n.name)).toEqual(['', 'b-folder', 'inner']),
    );
  });
});

describe('the folder store: merging rows by node id and feed position', () => {
  it('inserts a new row at its sorted place', () => {
    // Act
    folderStore.upsert(row('c.txt'));

    // Assert
    expect(names()).toEqual(['b-folder', 'b.txt', 'c.txt', 'd.txt']);
  });

  it('replaces a row of the same node when the feed position is not older', () => {
    // Arrange
    const held = folderStore.items.find((i) => i.name === 'b.txt');

    // Act
    folderStore.upsert({
      ...(held as NonNullable<typeof held>),
      size: 999,
      seq: (held?.seq ?? 0) + 1,
    });

    // Assert
    expect(folderStore.items.find((i) => i.name === 'b.txt')?.size).toBe(999);
    expect(names()).toHaveLength(3);
  });

  it('ignores a stale row with a lower feed position', () => {
    // Arrange
    const held = folderStore.items.find((i) => i.name === 'b.txt');
    folderStore.upsert({ ...(held as NonNullable<typeof held>), seq: 10, size: 1 });

    // Act
    folderStore.upsert({ ...(held as NonNullable<typeof held>), seq: 9, size: 2 });

    // Assert
    expect(folderStore.items.find((i) => i.name === 'b.txt')?.size).toBe(1);
  });

  it('removes a row on a tombstone', () => {
    // Arrange
    const held = folderStore.items.find((i) => i.name === 'b.txt');

    // Act
    folderStore.upsert({ ...(held as NonNullable<typeof held>), state: 'tombstone', seq: 99 });

    // Assert
    expect(names()).toEqual(['b-folder', 'd.txt']);
  });

  it('ignores a row of another folder', () => {
    // Act
    folderStore.upsert(
      makeNode({ name: 'elsewhere', kind: 'file', parent_id: 'some-other-folder' }),
    );

    // Assert
    expect(names()).toHaveLength(3);
  });

  it('leaves a new row that sorts after the last loaded one to paging while more pages follow', () => {
    // Arrange
    folderStore.next = 'c:3';

    // Act
    folderStore.upsert(row('zzz.txt'));

    // Assert
    expect(names()).not.toContain('zzz.txt');
  });

  it('records a pushed thumbnail state for a content hash', () => {
    // Act
    folderStore.setThumbState('h1', 'ready');

    // Assert
    expect(folderStore.thumbs.h1).toBe('ready');
  });
});
