import { describe, expect, it, vi } from 'vitest';
import { FakeSocket } from '../../../tests/support/fakeSocket';
import { realtimeUrl } from '.';
import {
  connectLive,
  frame,
  hello,
  installClientHooks,
  rig,
  settle,
  subscribeHex,
} from './client.test-support';

installClientHooks();

describe('the realtime client: first connection, in the order of the contract', () => {
  it('waits for Hello, then reads the resume point, then the listing, then subscribes at that point', async () => {
    // Arrange
    const r = rig();
    r.client.start();
    const socket = FakeSocket.last;
    socket.open();
    await settle();
    const beforeHello = [...r.log];

    // Act
    socket.deliver(hello());
    await settle();

    // Assert
    expect(beforeHello).toEqual([]);
    expect(r.log).toEqual(['loadHead', 'loadListing', `send ${subscribeHex(1n, 100n)}`]);
    expect(r.client.state).toBe('live');
    expect(socket.binaryType).toBe('arraybuffer');
  });

  it('does not send Subscribe until the listing has arrived', async () => {
    // Arrange
    const r = rig();
    let release: () => void = () => {};
    r.gates.listing = new Promise<void>((resolve) => (release = resolve));
    r.client.start();
    FakeSocket.last.open();
    FakeSocket.last.deliver(hello());
    await settle();

    // Assert while the listing is outstanding
    expect(r.log).toEqual(['loadHead', 'loadListing']);
    expect(r.client.state).toBe('connecting');

    // Act
    release();
    await settle();

    // Assert
    expect(r.log.at(-1)).toBe(`send ${subscribeHex(1n, 100n)}`);
  });

  it('connects to the same origin, path /api/v1/realtime and no query', () => {
    // Act
    const secure = realtimeUrl({ protocol: 'https:', host: 'drive.example.org' });
    const plain = realtimeUrl({ protocol: 'http:', host: 'localhost:5173' });

    // Assert
    expect(secure).toBe('wss://drive.example.org/api/v1/realtime');
    expect(plain).toBe('ws://localhost:5173/api/v1/realtime');
  });

  it('closes and asks for a reload when Hello names a protocol version it does not know', async () => {
    // Arrange
    const r = rig();
    r.client.start();
    FakeSocket.last.open();

    // Act
    FakeSocket.last.deliver(hello(2));
    await settle();
    await vi.advanceTimersByTimeAsync(120_000);

    // Assert
    expect(FakeSocket.last.closedWith?.code).toBe(1000);
    expect(r.handlers.upgrades).toBe(1);
    expect(r.client.state).toBe('failed');
    expect(FakeSocket.all).toHaveLength(1);
    expect(r.log).toEqual([]);
  });

  it('retries after the backoff when the first load fails', async () => {
    // Arrange
    const r = rig();
    r.handlers.loadHead = async () => null;
    r.client.start();
    FakeSocket.last.open();

    // Act
    FakeSocket.last.deliver(hello());
    await settle();

    // Assert
    expect(FakeSocket.last.closedWith).not.toBeNull();
    expect(r.client.state).toBe('reconnecting');
    await vi.advanceTimersByTimeAsync(1000);
    expect(FakeSocket.all).toHaveLength(2);
  });
});

describe('the realtime client: frames', () => {
  it('hands a Change to the app and a ThumbState to the thumbnail seam, without any REST call', async () => {
    // Arrange
    const r = rig();
    const fetchSpy = vi.fn();
    vi.stubGlobal('fetch', fetchSpy);
    const socket = await connectLive(r);

    // Act
    socket.deliver(frame('change_file'));
    socket.deliver(frame('thumb_state_ready'));

    // Assert
    expect(r.changes.map((c) => c.name)).toEqual(['IMG_0001.HEIC']);
    expect(r.thumbs).toEqual([
      ['2021222324252627' + '28292a2b2c2d2e2f' + '3031323334353637' + '38393a3b3c3d3e3f', 'ready'],
    ]);
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it('ignores a frame of an unknown type and keeps the socket', async () => {
    // Arrange
    const r = rig();
    const socket = await connectLive(r);

    // Act
    socket.deliver(Uint8Array.from([0x7e, 0x00]));

    // Assert
    expect(socket.closedWith).toBeNull();
    expect(r.client.state).toBe('live');
  });

  it.each([
    { what: 'a truncated frame', bytes: () => frame('bad_server_hello_truncated') },
    {
      what: 'a Change whose name length lies',
      bytes: () => frame('bad_server_change_name_length_lies'),
    },
    { what: 'an empty message', bytes: () => new Uint8Array() },
  ])(
    'closes with 1002 after $what and reconnects after the backoff, without reporting a bug',
    async ({ bytes }) => {
      // Arrange
      const r = rig();
      const socket = await connectLive(r);

      // Act
      socket.deliver(bytes());
      await vi.advanceTimersByTimeAsync(1000);

      // Assert
      expect(socket.closedWith?.code).toBe(1002);
      expect(FakeSocket.all).toHaveLength(2);
      expect(r.handlers.protocolErrors).toBe(0);
    },
  );

  it('treats a text message like any malformed frame', async () => {
    // Arrange
    const r = rig();
    const socket = await connectLive(r);

    // Act
    socket.deliverText('hello');

    // Assert
    expect(socket.closedWith?.code).toBe(1002);
  });
});
