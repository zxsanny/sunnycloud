import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FakeSocket } from '../../../tests/support/fakeSocket';
import { loadVectors } from '../../../tests/support/realtimeVectors';
import {
  RealtimeClient,
  backoffDelay,
  encodeSubscribe,
  parseCursor,
  realtimeUrl,
  type ChangeFrame,
  type RealtimeHandlers,
  type ResumePoint,
  type ThumbCode,
} from '.';

const vectors = loadVectors();
const frame = (name: string) => vectors.get(name) as Uint8Array;
const hex = (bytes: Uint8Array) =>
  Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
const subscribeHex = (epoch: bigint, seq: bigint) => hex(encodeSubscribe(epoch, seq));

/** Hello with a given ping interval (seconds): `20 | version | interval | max frame`. */
const hello = (version = 1, intervalS = 30) =>
  Uint8Array.from([0x20, version, 0, intervalS & 0xff, intervalS >> 8, 0x00, 0x04, 0, 0]);
/** A Change for one node: `seq` and the `end` flag are the parts the resume logic reads. */
function change(seq: number, end: boolean): Uint8Array {
  const bytes = Uint8Array.from(frame('change_root_child_minimal'));
  bytes[9] = seq;
  bytes[bytes.length - 1] = end ? 0x40 : 0x00;
  return bytes;
}
const reset = (reason: number, epoch: number, seq: number) =>
  Uint8Array.from([
    0x23,
    reason,
    epoch,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    seq & 0xff,
    seq >> 8,
    0,
    0,
    0,
    0,
    0,
    0,
  ]);
const pong = (nonce: number) => Uint8Array.from([0x24, nonce, 0, 0, 0]);

const HEAD: ResumePoint = { epoch: 1n, seq: 100n };

interface Rig {
  client: RealtimeClient;
  log: string[];
  changes: ChangeFrame[];
  thumbs: [string, ThumbCode][];
  handlers: RealtimeHandlers & {
    signedOut: number;
    protocolErrors: number;
    upgrades: number;
    checks: number;
    sessionAnswer: 'signed-out' | 'ok';
  };
  gates: { head: Promise<void> | null; listing: Promise<void> | null };
}

function rig(options: { random?: () => number } = {}): Rig {
  const log: string[] = [];
  const changes: ChangeFrame[] = [];
  const thumbs: [string, ThumbCode][] = [];
  const gates: Rig['gates'] = { head: null, listing: null };
  const handlers = {
    signedOut: 0,
    protocolErrors: 0,
    upgrades: 0,
    checks: 0,
    sessionAnswer: 'ok' as 'signed-out' | 'ok',
    checkSession: async () => {
      handlers.checks += 1;
      return handlers.sessionAnswer;
    },
    loadHead: async () => {
      log.push('loadHead');
      await gates.head;
      return HEAD;
    },
    loadListing: async () => {
      log.push('loadListing');
      await gates.listing;
      return true;
    },
    refreshPendingThumbs: async () => {
      log.push('refreshPendingThumbs');
    },
    onChange: (f: ChangeFrame) => changes.push(f),
    onThumbState: (h: string, s: ThumbCode) => thumbs.push([h, s]),
    onSignedOut: () => {
      handlers.signedOut += 1;
    },
    onProtocolError: () => {
      handlers.protocolErrors += 1;
    },
    onUpgradeRequired: () => {
      handlers.upgrades += 1;
    },
  };
  const client = new RealtimeClient({
    url: 'ws://localhost/api/v1/realtime',
    handlers,
    socket: (url) => {
      const socket = new FakeSocket(url);
      const send = socket.send.bind(socket);
      socket.send = (data) => {
        log.push(`send ${hex(data)}`);
        send(data);
      };
      return socket;
    },
    random: options.random ?? (() => 0),
  });
  return { client, log, changes, thumbs, handlers, gates };
}

/** Lets promises settle without moving the clock. */
const settle = () => vi.advanceTimersByTimeAsync(0);

async function connectLive(r: Rig, intervalS = 30) {
  r.client.start();
  const socket = FakeSocket.last;
  socket.open();
  socket.deliver(hello(1, intervalS));
  await settle();
  return socket;
}

beforeEach(() => {
  FakeSocket.reset();
  vi.useFakeTimers();
  vi.spyOn(console, 'warn').mockImplementation(() => {});
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

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

describe('the cursor of /changes/head', () => {
  it.each([
    ['1.42', { epoch: 1n, seq: 42n }],
    [
      '18446744073709551615.9007199254740993',
      { epoch: 18446744073709551615n, seq: 9007199254740993n },
    ],
  ])('reads %s', (text, point) => {
    // Act
    const parsed = parseCursor(text);

    // Assert
    expect(parsed).toEqual(point);
  });

  it.each(['', '1', '1.', '.1', '1.2.3', 'a.b', '-1.2', '1.2 '])('refuses %j', (text) => {
    // Act
    const parsed = parseCursor(text);

    // Assert
    expect(parsed).toBeNull();
  });
});
