import { afterEach, beforeEach, vi } from 'vitest';
import { FakeSocket } from '../../../tests/support/fakeSocket';
import { loadVectors } from '../../../tests/support/realtimeVectors';
import {
  RealtimeClient,
  encodeSubscribe,
  type ChangeFrame,
  type RealtimeHandlers,
  type ResumePoint,
  type ThumbCode,
} from '.';

const vectors = loadVectors();
export const frame = (name: string) => vectors.get(name) as Uint8Array;
const hex = (bytes: Uint8Array) =>
  Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
export const subscribeHex = (epoch: bigint, seq: bigint) => hex(encodeSubscribe(epoch, seq));

/** Hello with a given ping interval (seconds): `20 | version | interval | max frame`. */
export const hello = (version = 1, intervalS = 30) =>
  Uint8Array.from([0x20, version, 0, intervalS & 0xff, intervalS >> 8, 0x00, 0x04, 0, 0]);
/** A Change for one node: `seq` and the `end` flag are the parts the resume logic reads. */
export function change(seq: number, end: boolean): Uint8Array {
  const bytes = Uint8Array.from(frame('change_root_child_minimal'));
  bytes[9] = seq;
  bytes[bytes.length - 1] = end ? 0x40 : 0x00;
  return bytes;
}
export const reset = (reason: number, epoch: number, seq: number) =>
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
export const pong = (nonce: number) => Uint8Array.from([0x24, nonce, 0, 0, 0]);

const HEAD: ResumePoint = { epoch: 1n, seq: 100n };

export interface Rig {
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

export function rig(options: { random?: () => number } = {}): Rig {
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
export const settle = () => vi.advanceTimersByTimeAsync(0);

export async function connectLive(r: Rig, intervalS = 30) {
  r.client.start();
  const socket = FakeSocket.last;
  socket.open();
  socket.deliver(hello(1, intervalS));
  await settle();
  return socket;
}

/** Gives every test of the calling file a fresh socket list, a fake clock and a quiet console. */
export function installClientHooks() {
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
}
