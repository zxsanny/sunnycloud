import { describe, expect, it, vi } from 'vitest';
import { FakeSocket } from '../../../tests/support/fakeSocket';
import { backoffDelay } from '.';
import {
  change,
  connectLive,
  hello,
  installClientHooks,
  reset,
  rig,
  settle,
  subscribeHex,
} from './client.test-support';

installClientHooks();

describe('the realtime client: reconnect and the resume point', () => {
  it('resubscribes at the last Change that ended its batch and asks once for pending thumbnails', async () => {
    // Arrange
    const r = rig();
    const first = await connectLive(r);
    first.deliver(change(5, true));
    first.deliver(change(6, false));
    first.deliver(change(6, true));
    first.deliver(change(7, false));
    r.log.length = 0;

    // Act
    first.serverClose(1006);
    await vi.advanceTimersByTimeAsync(1000);
    const second = FakeSocket.last;
    second.open();
    second.deliver(hello());
    await settle();

    // Assert
    expect(second).not.toBe(first);
    expect(r.log).toEqual([`send ${subscribeHex(1n, 6n)}`, 'refreshPendingThumbs']);
    expect(r.client.state).toBe('live');
  });

  it('does not move the resume point on a Change without the end flag', async () => {
    // Arrange
    const r = rig();
    const first = await connectLive(r);
    first.deliver(change(9, false));
    r.log.length = 0;

    // Act
    first.serverClose(1001);
    await vi.advanceTimersByTimeAsync(1000);
    FakeSocket.last.open();
    FakeSocket.last.deliver(hello());
    await settle();

    // Assert: still the head of the first load
    expect(r.log[0]).toBe(`send ${subscribeHex(1n, 100n)}`);
  });

  it('does not repeat the first load or the thumbnail call on the first connection', async () => {
    // Arrange
    const r = rig();

    // Act
    await connectLive(r);

    // Assert
    expect(r.log.filter((l) => l === 'refreshPendingThumbs')).toHaveLength(0);
    expect(r.log.filter((l) => l === 'loadListing')).toHaveLength(1);
  });

  it.each([1001, 1006, 1011, 1013, 4408, 4429, 1000])(
    'reconnects after close code %i',
    async (code) => {
      // Arrange
      const r = rig();
      const socket = await connectLive(r);

      // Act
      socket.serverClose(code);
      await vi.advanceTimersByTimeAsync(1000);

      // Assert
      expect(FakeSocket.all).toHaveLength(2);
      expect(r.handlers.signedOut + r.handlers.protocolErrors).toBe(0);
    },
  );

  it('goes to sign-in on 4401 and never reconnects', async () => {
    // Arrange
    const r = rig();
    const socket = await connectLive(r);

    // Act
    socket.serverClose(4401);
    await vi.advanceTimersByTimeAsync(600_000);

    // Assert
    expect(r.handlers.signedOut).toBe(1);
    expect(r.client.state).toBe('signed-out');
    expect(FakeSocket.all).toHaveLength(1);
  });

  it('reports a protocol error once on 1002 and does not loop', async () => {
    // Arrange
    const r = rig();
    const socket = await connectLive(r);

    // Act
    socket.serverClose(1002);
    await vi.advanceTimersByTimeAsync(600_000);

    // Assert
    expect(r.handlers.protocolErrors).toBe(1);
    expect(r.client.state).toBe('failed');
    expect(FakeSocket.all).toHaveLength(1);
  });

  it('stops for good when asked, and closes the socket', async () => {
    // Arrange
    const r = rig();
    const socket = await connectLive(r);

    // Act
    r.client.stop();
    await vi.advanceTimersByTimeAsync(600_000);

    // Assert
    expect(socket.closedWith?.code).toBe(1000);
    expect(FakeSocket.all).toHaveLength(1);
    expect(r.client.state).toBe('idle');
  });
});

describe('the realtime client: Reset', () => {
  it('reloads the listing once over REST and subscribes again at the head of the Reset', async () => {
    // Arrange
    const r = rig();
    const socket = await connectLive(r);
    r.log.length = 0;

    // Act
    socket.deliver(reset(2, 3, 900));
    await settle();

    // Assert
    expect(r.log).toEqual(['loadListing', `send ${subscribeHex(3n, 900n)}`]);
    expect(socket.closedWith).toBeNull();
  });

  it('resumes at the head of the Reset after a later drop', async () => {
    // Arrange
    const r = rig();
    const socket = await connectLive(r);
    socket.deliver(reset(4, 3, 900));
    await settle();
    r.log.length = 0;

    // Act
    socket.serverClose(1006);
    await vi.advanceTimersByTimeAsync(1000);
    FakeSocket.last.open();
    FakeSocket.last.deliver(hello());
    await settle();

    // Assert
    expect(r.log[0]).toBe(`send ${subscribeHex(3n, 900n)}`);
  });

  it('subscribes only once for two Resets that arrive while the listing is loading, at the latest head', async () => {
    // Arrange
    const r = rig();
    const socket = await connectLive(r);
    let release: () => void = () => {};
    r.gates.listing = new Promise<void>((resolve) => (release = resolve));
    r.log.length = 0;

    // Act
    socket.deliver(reset(4, 3, 900));
    socket.deliver(reset(4, 3, 950));
    await settle();
    release();
    await settle();
    r.gates.listing = null;
    await settle();

    // Assert
    const subscribes = r.log.filter((l) => l.startsWith('send 01'));
    expect(subscribes.at(-1)).toBe(`send ${subscribeHex(3n, 950n)}`);
  });
});

describe('the reconnect backoff', () => {
  it('starts at 1 second, doubles, and never passes 30 seconds, with up to 25 percent jitter', () => {
    // Act
    const low = Array.from({ length: 10 }, (_, n) => backoffDelay(n, () => 0));
    const high = Array.from({ length: 10 }, (_, n) => backoffDelay(n, () => 1));

    // Assert
    expect(low).toEqual([1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000, 30000, 30000]);
    expect(high.every((d, n) => d >= low[n]! && d <= 30_000)).toBe(true);
    expect(high[0]).toBe(1250);
  });

  it('waits 1, 2, then 4 seconds for successive failed attempts and starts over after a Hello', async () => {
    // Arrange
    const r = rig();
    r.client.start();
    FakeSocket.last.serverClose(1006);
    await vi.advanceTimersByTimeAsync(999);
    expect(FakeSocket.all).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    FakeSocket.last.serverClose(1006);
    await vi.advanceTimersByTimeAsync(1999);
    expect(FakeSocket.all).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(FakeSocket.all).toHaveLength(3);

    // Act: this one gets a Hello, so the next drop waits 1 second again
    FakeSocket.last.open();
    FakeSocket.last.deliver(hello());
    await settle();
    FakeSocket.last.serverClose(1006);
    await vi.advanceTimersByTimeAsync(1000);

    // Assert
    expect(FakeSocket.all).toHaveLength(4);
  });
});
