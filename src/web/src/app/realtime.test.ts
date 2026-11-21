import { render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FakeSocket } from '../../tests/support/fakeSocket';
import {
  changeBytes,
  helloBytes,
  resetBytes,
  thumbStateBytes,
} from '../../tests/support/realtimeFrames';
import { TreeServer, type Node } from '../../tests/support/treeServer';
import { App, handleApiError } from '.';
import { api } from '$lib/api';
import { folderStore, viewPreference } from '$lib/files';
import { notices } from '$lib/notices';
import { encodeSubscribe } from '$lib/realtime';
import { navigate, route } from '$lib/router';
import { session } from '$lib/session';

let server: TreeServer;
let fetchCount = 0;

const hashOf = (node: Node) => node.content_hash as string;
const subscribeHex = (epoch: bigint, seq: bigint) =>
  Array.from(encodeSubscribe(epoch, seq), (b) => b.toString(16).padStart(2, '0')).join('');
const names = () => folderStore.items.map((i) => i.name);

beforeEach(() => {
  FakeSocket.reset();
  fetchCount = 0;
  server = new TreeServer();
  vi.stubGlobal('fetch', (request: Request) => {
    fetchCount += 1;
    return server.fetch(request);
  });
  session.status = 'idle';
  session.account = null;
  folderStore.thumbs = {};
  notices.items = [];
  localStorage.clear();
  viewPreference.mode = 'list';
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

async function openPage(path = '/') {
  navigate(path, { replace: true });
  const view = render(App);
  await waitFor(() => expect(FakeSocket.all.length).toBeGreaterThan(0));
  return view;
}

/** The page loaded its folder, the socket opened and said Hello, and the first load finished. */
async function goLive(view?: Awaited<ReturnType<typeof openPage>>) {
  const socket = FakeSocket.last;
  socket.open();
  socket.deliver(helloBytes());
  await waitFor(() => expect(socket.sent.length).toBeGreaterThan(0));
  void view;
  return socket;
}

const methods = () =>
  server.requests.map((r) => `${r.method} ${r.path.replace(/[0-9a-f]{8}-[0-9a-f-]{27}/g, ':id')}`);

describe('the realtime wiring: first load order', () => {
  it('reads the folder only after Hello, the resume point before the listing, and subscribes at that point last', async () => {
    // Arrange
    server.add(server.root, { name: 'a.txt', kind: 'file' });
    await openPage();
    await screen.findByText('a.txt');
    const requestsBeforeHello = server.requests.length;
    const socket = FakeSocket.last;
    socket.open();

    // Act
    socket.deliver(helloBytes());
    await waitFor(() => expect(socket.sentHex).toEqual([subscribeHex(1n, 100n)]));

    // Assert
    const afterHello = methods().slice(requestsBeforeHello);
    expect(afterHello.slice(0, 2)).toEqual([
      'GET /api/v1/changes/head',
      'GET /api/v1/nodes/:id/children',
    ]);
    expect(socket.url).toBe(`ws://${location.host}/api/v1/realtime`);
  });

  it('subscribes at the head the server named', async () => {
    // Arrange
    server.head = '7.4242';
    await openPage();

    // Act
    const socket = await goLive();

    // Assert
    expect(socket.sentHex).toEqual([subscribeHex(7n, 4242n)]);
  });
});

describe('the realtime wiring: pushed frames', () => {
  it('adds a row for a live Change without any REST call', async () => {
    // Arrange
    const view = await openPage();
    await screen.findByText('This folder is empty');
    const socket = await goLive(view);
    const before = fetchCount;

    // Act
    socket.deliver(
      changeBytes({
        seq: 101,
        nodeId: '0190dddd-0000-7000-8000-000000000001',
        parentId: server.root.node_id,
        name: 'fresh.txt',
        size: 12,
        contentHash: 'ab'.repeat(32),
        mediaType: 'text/plain',
      }),
    );

    // Assert
    expect(await screen.findByText('fresh.txt')).toBeInTheDocument();
    expect(fetchCount).toBe(before);
  });

  it('turns a skeleton into an image when a ThumbState frame arrives', async () => {
    // Arrange
    const photo = server.add(server.root, {
      name: 'late.jpg',
      kind: 'file',
      media_type: 'image/jpeg',
    });
    server.thumbStates[hashOf(photo)] = 'pending';
    const view = await openPage();
    const row = (await screen.findByText('late.jpg')).closest('tr') as HTMLElement;
    await waitFor(() => expect(within(row).getByText('Preview pending')).toBeInTheDocument());
    const socket = await goLive(view);
    const before = fetchCount;

    // Act
    socket.deliver(thumbStateBytes(hashOf(photo), 2));

    // Assert
    await waitFor(() => expect(row.querySelector('img')).not.toBeNull());
    expect(fetchCount).toBe(before);
  });

  it('shows a failed preview as no preview when ThumbState says failed', async () => {
    // Arrange
    const photo = server.add(server.root, {
      name: 'bad.jpg',
      kind: 'file',
      media_type: 'image/jpeg',
    });
    server.thumbStates[hashOf(photo)] = 'pending';
    const view = await openPage();
    const row = (await screen.findByText('bad.jpg')).closest('tr') as HTMLElement;
    const socket = await goLive(view);

    // Act
    socket.deliver(thumbStateBytes(hashOf(photo), 3));

    // Assert
    await waitFor(() => expect(within(row).getByText('No preview')).toBeInTheDocument());
  });

  it.each([
    { code: 1 as const, shows: 'Preview pending' },
    { code: 2 as const, shows: null },
    { code: 3 as const, shows: 'No preview' },
  ])(
    'takes the thumbnail state of a Change (thumb $code) for its content hash',
    async ({ code, shows }) => {
      // Arrange
      const view = await openPage();
      await screen.findByText('This folder is empty');
      const socket = await goLive(view);
      const hash = 'cd'.repeat(32);

      // Act
      socket.deliver(
        changeBytes({
          seq: 102,
          nodeId: '0190dddd-0000-7000-8000-000000000002',
          parentId: server.root.node_id,
          name: 'pic.jpg',
          mediaType: 'image/jpeg',
          contentHash: hash,
          thumb: code,
        }),
      );

      // Assert
      const row = (await screen.findByText('pic.jpg')).closest('tr') as HTMLElement;
      if (shows) expect(within(row).getByText(shows)).toBeInTheDocument();
      else await waitFor(() => expect(row.querySelector('img')).not.toBeNull());
    },
  );

  it('removes a row on a tombstone', async () => {
    // Arrange
    const file = server.add(server.root, { name: 'gone.txt', kind: 'file' });
    const view = await openPage();
    await screen.findByText('gone.txt');
    const socket = await goLive(view);

    // Act
    socket.deliver(
      changeBytes({
        seq: 103,
        nodeId: file.node_id,
        parentId: server.root.node_id,
        name: 'gone.txt',
        deleted: true,
      }),
    );

    // Assert
    await waitFor(() => expect(screen.queryByText('gone.txt')).toBeNull());
  });

  it('removes a row whose node moved to another folder, and ignores a node that is not in this folder', async () => {
    // Arrange
    const file = server.add(server.root, { name: 'moving.txt', kind: 'file' });
    const view = await openPage();
    await screen.findByText('moving.txt');
    const socket = await goLive(view);

    // Act
    socket.deliver(
      changeBytes({
        seq: 104,
        nodeId: file.node_id,
        parentId: '0190dddd-0000-7000-8000-0000000000ff',
        name: 'moving.txt',
      }),
    );
    socket.deliver(
      changeBytes({
        seq: 105,
        nodeId: '0190dddd-0000-7000-8000-000000000003',
        parentId: '0190dddd-0000-7000-8000-0000000000ff',
        name: 'elsewhere.txt',
      }),
    );

    // Assert
    await waitFor(() => expect(screen.queryByText('moving.txt')).toBeNull());
    expect(names()).not.toContain('elsewhere.txt');
  });

  it('updates a renamed row in place and ignores an older state of the same node', async () => {
    // Arrange
    const file = server.add(server.root, { name: 'old-name.txt', kind: 'file' });
    file.seq = 50;
    const view = await openPage();
    await screen.findByText('old-name.txt');
    const socket = await goLive(view);

    // Act
    socket.deliver(
      changeBytes({
        seq: 60,
        nodeId: file.node_id,
        parentId: server.root.node_id,
        name: 'new-name.txt',
      }),
    );
    await screen.findByText('new-name.txt');
    socket.deliver(
      changeBytes({
        seq: 55,
        nodeId: file.node_id,
        parentId: server.root.node_id,
        name: 'stale-name.txt',
      }),
    );

    // Assert
    expect(names()).toEqual(['new-name.txt']);
  });
});

describe('the realtime wiring: connection loss and recovery', () => {
  it('resubscribes at the last applied position and asks once for the pending previews, with no listing read', async () => {
    // Arrange
    const photo = server.add(server.root, {
      name: 'p.jpg',
      kind: 'file',
      media_type: 'image/jpeg',
    });
    server.thumbStates[hashOf(photo)] = 'pending';
    const view = await openPage();
    await screen.findByText('p.jpg');
    const first = await goLive(view);
    first.deliver(
      changeBytes({
        seq: 120,
        nodeId: '0190dddd-0000-7000-8000-000000000004',
        parentId: server.root.node_id,
        name: 'x.txt',
      }),
    );
    await screen.findByText('x.txt');
    const marker = server.requests.length;

    // Act
    first.serverClose(1006);
    await waitFor(() => expect(FakeSocket.all).toHaveLength(2), { timeout: 4000 });
    const second = FakeSocket.last;
    second.open();
    second.deliver(helloBytes());
    await waitFor(() => expect(second.sentHex).toEqual([subscribeHex(1n, 120n)]));

    // Assert
    await waitFor(() => expect(methods().slice(marker)).toEqual(['POST /api/v1/thumbnail-states']));
    expect(server.requests.slice(marker)[0]?.body).toEqual({ content_hashes: [hashOf(photo)] });
  });

  it('reloads the listing once over REST on Reset and subscribes at the head the frame names', async () => {
    // Arrange
    const view = await openPage();
    await screen.findByText('This folder is empty');
    const socket = await goLive(view);
    server.add(server.root, { name: 'missed.txt', kind: 'file' });
    const marker = server.requests.length;

    // Act
    socket.deliver(resetBytes(2, 1n, 900n));

    // Assert
    expect(await screen.findByText('missed.txt')).toBeInTheDocument();
    await waitFor(() => expect(socket.sentHex.at(-1)).toBe(subscribeHex(1n, 900n)));
    expect(methods().slice(marker)).toEqual(['GET /api/v1/nodes/:id/children']);
  });

  it('sends the user to sign-in on close code 4401 and opens no second socket', async () => {
    // Arrange
    api.setErrorHandler(handleApiError);
    const view = await openPage('/folders/' + server.root.node_id);
    await screen.findByText('This folder is empty');
    const socket = await goLive(view);

    // Act
    socket.serverClose(4401);

    // Assert
    await waitFor(() => expect(route.path).toBe('/login'));
    expect(FakeSocket.all).toHaveLength(1);
    expect(notices.items.map((n) => n.title)).toContain('Your session ended.');
  });

  it('tells the user to reload on close code 1002 and does not reconnect', async () => {
    // Arrange
    const view = await openPage();
    await screen.findByText('This folder is empty');
    const socket = await goLive(view);

    // Act
    socket.serverClose(1002);

    // Assert
    await waitFor(() =>
      expect(notices.items.map((n) => n.title)).toContain('Live updates stopped.'),
    );
    expect(FakeSocket.all).toHaveLength(1);
  });
});

describe('the connection chip', () => {
  it('says Reconnecting until the socket is live, Live after Subscribe, and Reconnecting again after a drop', async () => {
    // Arrange
    const view = await openPage();
    await screen.findByText('This folder is empty');
    expect(screen.getByRole('status', { name: 'Live updates: Reconnecting' })).toBeInTheDocument();

    // Act
    const socket = await goLive(view);

    // Assert
    expect(await screen.findByRole('status', { name: 'Live updates: Live' })).toHaveTextContent(
      'Live',
    );
    socket.serverClose(1006);
    expect(
      await screen.findByRole('status', { name: 'Live updates: Reconnecting' }),
    ).toBeInTheDocument();
  });

  it('says Offline when the session ended', async () => {
    // Arrange
    const view = await openPage();
    await screen.findByText('This folder is empty');
    const socket = await goLive(view);

    // Act
    socket.serverClose(1002);

    // Assert
    expect(
      await screen.findByRole('status', { name: 'Live updates: Offline' }),
    ).toBeInTheDocument();
  });
});
