import { describe, expect, it, vi } from 'vitest';
import { FakeSocket } from '../../../tests/support/fakeSocket';
import { connectLive, hello, installClientHooks, pong, rig, settle } from './client.test-support';

installClientHooks();

describe('the realtime client: keepalive and dead sockets', () => {
  it('sends a Ping with a nonce every interval and stays up while Pongs come back', async () => {
    // Arrange
    const r = rig();
    const socket = await connectLive(r);
    socket.autoPong = true;

    // Act
    await vi.advanceTimersByTimeAsync(30_000 * 4);

    // Assert
    const pings = socket.sentHex.filter((h) => h.startsWith('02'));
    expect(pings).toEqual(['0201000000', '0202000000', '0203000000', '0204000000']);
    expect(socket.closedWith).toBeNull();
    expect(FakeSocket.all).toHaveLength(1);
  });

  it('closes and reconnects when no Pong comes within 10 seconds', async () => {
    // Arrange
    const r = rig();
    const socket = await connectLive(r);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(socket.sentHex.at(-1)).toBe('0201000000');

    // Act
    await vi.advanceTimersByTimeAsync(9_999);
    expect(socket.closedWith).toBeNull();
    await vi.advanceTimersByTimeAsync(1);

    // Assert
    expect(socket.closedWith).not.toBeNull();
    expect(r.client.state).toBe('reconnecting');
    await vi.advanceTimersByTimeAsync(1000);
    expect(FakeSocket.all).toHaveLength(2);
  });

  it('ignores a Pong with another nonce', async () => {
    // Arrange
    const r = rig();
    const socket = await connectLive(r);
    await vi.advanceTimersByTimeAsync(30_000);

    // Act
    socket.deliver(pong(99));
    await vi.advanceTimersByTimeAsync(10_000);

    // Assert
    expect(socket.closedWith).not.toBeNull();
  });

  it('treats 90 seconds of silence as a dead socket', async () => {
    // Arrange
    const r = rig();
    const socket = await connectLive(r, 3600);

    // Act
    await vi.advanceTimersByTimeAsync(89_999);
    expect(socket.closedWith).toBeNull();
    await vi.advanceTimersByTimeAsync(1);

    // Assert
    expect(socket.closedWith).not.toBeNull();
    await vi.advanceTimersByTimeAsync(1000);
    expect(FakeSocket.all).toHaveLength(2);
  });

  it('treats a socket that opens but never says Hello as dead after 10 seconds', async () => {
    // Arrange
    const r = rig();
    r.client.start();
    FakeSocket.last.open();

    // Act
    await vi.advanceTimersByTimeAsync(10_000);

    // Assert
    expect(FakeSocket.last.closedWith).not.toBeNull();
    await vi.advanceTimersByTimeAsync(1000);
    expect(FakeSocket.all).toHaveLength(2);
  });

  it('asks the server for nothing on any timer: no fetch and no REST read in minutes of idle time', async () => {
    // Arrange
    const r = rig();
    const fetchSpy = vi.fn();
    vi.stubGlobal('fetch', fetchSpy);
    const socket = await connectLive(r);
    socket.autoPong = true;
    const readsAfterFirstLoad = r.log.filter((l) => l.startsWith('load')).length;

    // Act
    await vi.advanceTimersByTimeAsync(30 * 60 * 1000);

    // Assert
    expect(fetchSpy).not.toHaveBeenCalled();
    expect(r.log.filter((l) => l.startsWith('load')).length).toBe(readsAfterFirstLoad);
    expect(r.log.filter((l) => l === 'refreshPendingThumbs')).toHaveLength(0);
    expect(socket.sentHex.every((h) => h.startsWith('02') || h.startsWith('01'))).toBe(true);
  });
});

/** A connection attempt that ends before Hello, as a refused handshake does (close code 1006). */
async function failHandshake() {
  FakeSocket.last.serverClose(1006);
  await vi.advanceTimersByTimeAsync(30_000);
}

describe('the session check after refused handshakes', () => {
  it('checks the session once after three attempts in a row that never reached Hello, and not before', async () => {
    // Arrange
    const r = rig();
    r.client.start();

    // Act
    await failHandshake();
    await failHandshake();
    const afterTwo = r.handlers.checks;
    await failHandshake();

    // Assert
    expect(afterTwo).toBe(0);
    expect(r.handlers.checks).toBe(1);
  });

  it('does not check again while the attempts keep failing', async () => {
    // Arrange
    const r = rig();
    r.client.start();

    // Act
    for (let i = 0; i < 8; i += 1) await failHandshake();

    // Assert
    expect(r.handlers.checks).toBe(1);
  });

  it('goes to sign-in when the check finds the session gone, and stops reconnecting', async () => {
    // Arrange
    const r = rig();
    r.handlers.sessionAnswer = 'signed-out';
    r.client.start();
    await failHandshake();
    await failHandshake();

    // Act
    FakeSocket.last.serverClose(1006);
    await vi.advanceTimersByTimeAsync(600_000);

    // Assert
    expect(r.client.state).toBe('signed-out');
    expect(FakeSocket.all).toHaveLength(3);
    expect(r.handlers.checks).toBe(1);
  });

  it('keeps reconnecting without a second check when the session is fine', async () => {
    // Arrange
    const r = rig();
    r.client.start();
    await failHandshake();
    await failHandshake();
    await failHandshake();

    // Act
    await failHandshake();
    await failHandshake();

    // Assert
    expect(r.handlers.checks).toBe(1);
    expect(FakeSocket.all.length).toBeGreaterThan(4);
    expect(r.client.state).not.toBe('signed-out');
  });

  it('checks again only after a Hello has happened in between', async () => {
    // Arrange
    const r = rig();
    r.client.start();
    await failHandshake();
    await failHandshake();
    await failHandshake();
    expect(r.handlers.checks).toBe(1);

    // Act: a connection that gets its Hello resets the count
    FakeSocket.last.open();
    FakeSocket.last.deliver(hello());
    await settle();
    await failHandshake();
    await failHandshake();
    const beforeThird = r.handlers.checks;
    await failHandshake();

    // Assert: the drop after the Hello is not a failed handshake, so three more are needed
    expect(beforeThird).toBe(1);
    expect(r.handlers.checks).toBe(1);
    await failHandshake();
    expect(r.handlers.checks).toBe(2);
  });

  it('never checks while connections succeed, however often they drop after their Hello', async () => {
    // Arrange
    const r = rig();
    r.client.start();

    // Act
    for (let i = 0; i < 6; i += 1) {
      FakeSocket.last.open();
      FakeSocket.last.deliver(hello());
      await settle();
      FakeSocket.last.serverClose(1006);
      await vi.advanceTimersByTimeAsync(30_000);
    }

    // Assert
    expect(r.handlers.checks).toBe(0);
  });

  it('counts a socket that opens but never says Hello as a failed handshake', async () => {
    // Arrange
    const r = rig();
    r.client.start();

    // Act
    for (let i = 0; i < 3; i += 1) {
      FakeSocket.last.open();
      await vi.advanceTimersByTimeAsync(10_000);
      await vi.advanceTimersByTimeAsync(30_000);
    }

    // Assert
    expect(r.handlers.checks).toBe(1);
  });
});
