import {
  BinaryPacker,
  bool,
  bytes,
  flags,
  scheme,
  u16,
  u32,
  u64,
  u8,
  utf8,
  type SchemeHandler,
} from 'packbin';

/**
 * The frames of the realtime channel (contract realtime.md). A frame is one binary message, byte 0 is the scheme
 * number, integers are little-endian, `u64` is `bigint`. packbin reads member names from the source text of the
 * accessor lambdas, so every accessor below is the plain form `(x: Row) => x.member`, and a test runs the decoder
 * against a minified build to keep it that way.
 */

// Row shapes: the member names are the field names of the contract.
class SubscribeRow {
  epoch = 0n;
  seq = 0n;
}
class PingRow {
  nonce = 0;
}
class HelloRow {
  version = 0;
  ping_interval_s = 0;
  max_client_frame = 0;
}
class ChangeRow {
  epoch = 0n;
  seq = 0n;
  node_id: Uint8Array = new Uint8Array(16);
  kind = 0;
  thumb = 0;
  name = '';
  parent_id: Uint8Array | null = null;
  revision_id: Uint8Array | null = null;
  size: bigint | null = null;
  content_hash: Uint8Array | null = null;
  media_type: string | null = null;
  deleted: boolean | null = null;
  end: boolean | null = null;
}
class ThumbStateRow {
  content_hash: Uint8Array = new Uint8Array(32);
  state = 0;
}
class ResetRow {
  reason = 0;
  epoch = 0n;
  seq = 0n;
}

export const schemes = {
  subscribe: scheme<SubscribeRow>(
    0x01,
    u64(0, (x: SubscribeRow) => x.epoch),
    u64(1, (x: SubscribeRow) => x.seq),
  ),
  ping: scheme<PingRow>(
    0x02,
    u32(0, (x: PingRow) => x.nonce),
  ),
  hello: scheme<HelloRow>(
    0x20,
    u16(0, (x: HelloRow) => x.version),
    u16(1, (x: HelloRow) => x.ping_interval_s),
    u32(2, (x: HelloRow) => x.max_client_frame),
  ),
  change: scheme<ChangeRow>(
    0x21,
    u64(0, (x: ChangeRow) => x.epoch),
    u64(1, (x: ChangeRow) => x.seq),
    bytes(2, (x: ChangeRow) => x.node_id, 16),
    u8(3, (x: ChangeRow) => x.kind),
    u8(4, (x: ChangeRow) => x.thumb),
    utf8(5, (x: ChangeRow) => x.name),
    flags(6, [
      bytes(6, (x: ChangeRow) => x.parent_id, 16),
      bytes(7, (x: ChangeRow) => x.revision_id, 16),
      u64(8, (x: ChangeRow) => x.size),
      bytes(9, (x: ChangeRow) => x.content_hash, 32),
      utf8(10, (x: ChangeRow) => x.media_type),
      bool(11, (x: ChangeRow) => x.deleted),
      bool(12, (x: ChangeRow) => x.end),
    ]),
  ),
  thumbState: scheme<ThumbStateRow>(
    0x22,
    bytes(0, (x: ThumbStateRow) => x.content_hash, 32),
    u8(1, (x: ThumbStateRow) => x.state),
  ),
  reset: scheme<ResetRow>(
    0x23,
    u8(0, (x: ResetRow) => x.reason),
    u64(1, (x: ResetRow) => x.epoch),
    u64(2, (x: ResetRow) => x.seq),
  ),
  pong: scheme<PingRow>(
    0x24,
    u32(0, (x: PingRow) => x.nonce),
  ),
};

export type ThumbCode = 'none' | 'pending' | 'ready' | 'failed';

export interface HelloFrame {
  type: 'hello';
  version: number;
  pingIntervalS: number;
  maxClientFrame: number;
}
export interface ChangeFrame {
  type: 'change';
  epoch: bigint;
  seq: bigint;
  nodeId: string;
  nodeKind: 'file' | 'folder';
  /** 0 none, 1 pending, 2 ready, 3 failed; an unknown value is none. */
  thumb: ThumbCode;
  name: string;
  parentId: string | null;
  revisionId: string | null;
  size: bigint | null;
  /** 64 lower-case hex digits, as in the REST API. */
  contentHash: string | null;
  mediaType: string | null;
  deleted: boolean;
  /** The last item of its `seq` batch: only then may the resume point move to (epoch, seq). */
  end: boolean;
}
export interface ThumbStateFrame {
  type: 'thumbState';
  contentHash: string;
  state: ThumbCode;
}
export interface ResetFrame {
  type: 'reset';
  /** 1 epoch changed, 2 below the purge horizon, 3 cursor ahead, 4 lagged. */
  reason: number;
  epoch: bigint;
  seq: bigint;
}
export interface PongFrame {
  type: 'pong';
  nonce: number;
}
export type ServerFrame = HelloFrame | ChangeFrame | ThumbStateFrame | ResetFrame | PongFrame;

export interface SubscribeFrame {
  type: 'subscribe';
  epoch: bigint;
  seq: bigint;
}
export interface PingFrame {
  type: 'ping';
  nonce: number;
}
export type ClientFrame = SubscribeFrame | PingFrame;

/** Why a message was not decoded. A refusal is a value: bytes from the network never throw. */
export type Refusal =
  | { reason: 'empty' }
  | { reason: 'unknown_type'; type: number }
  | { reason: 'malformed'; type: number; field: string; needed: number; left: number }
  | { reason: 'invalid_value'; type: number; field: string };

export type Decoded<F> = { ok: true; frame: F } | { ok: false; refusal: Refusal };

const hex = (raw: Uint8Array) => Array.from(raw, (b) => b.toString(16).padStart(2, '0')).join('');

function uuid(raw: Uint8Array): string {
  const h = hex(raw);
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

const THUMBS: ThumbCode[] = ['none', 'pending', 'ready', 'failed'];
const thumbOf = (code: number): ThumbCode => THUMBS[code] ?? 'none';

function present<T>(value: T | null | undefined): value is T {
  return value !== null && value !== undefined;
}

type Handler = SchemeHandler<object>;

function dispatch(message: ArrayBuffer | Uint8Array, handlers: [Handler, ...Handler[]]) {
  const raw = message instanceof Uint8Array ? message : new Uint8Array(message);
  if (raw.length === 0) return { ok: false, refusal: { reason: 'empty' } } as const;
  const result = BinaryPacker.unpack(raw, ...handlers);
  if (result.ok) return { ok: true } as const;
  if ('actual' in result) {
    return { ok: false, refusal: { reason: 'unknown_type', type: result.actual } } as const;
  }
  // Only the short and type-mismatch refusals exist here: no scheme declares parity bytes.
  const short = 'field' in result ? result : { field: '', needed: 0, left: 0 };
  return {
    ok: false,
    refusal: {
      reason: 'malformed',
      type: raw[0] ?? 0,
      field: short.field,
      needed: short.needed,
      left: short.left,
    },
  } as const;
}

/** Decodes one message from the server. An unknown first byte is a refusal the caller ignores (contract). */
export function decodeServerFrame(message: ArrayBuffer | Uint8Array): Decoded<ServerFrame> {
  let frame: ServerFrame | null = null;
  let invalid: Refusal | null = null;
  const take = (next: ServerFrame) => {
    frame = next;
  };
  const refuse = (type: number, field: string) => {
    invalid = { reason: 'invalid_value', type, field };
  };
  const handlers = [
    schemes.hello.on((row) =>
      take({
        type: 'hello',
        version: row.version,
        pingIntervalS: row.ping_interval_s,
        maxClientFrame: row.max_client_frame,
      }),
    ),
    schemes.change.on((row) => {
      if (row.kind !== 0 && row.kind !== 1) return refuse(0x21, 'kind');
      take({
        type: 'change',
        epoch: row.epoch,
        seq: row.seq,
        nodeId: uuid(row.node_id),
        nodeKind: row.kind === 0 ? 'file' : 'folder',
        thumb: thumbOf(row.thumb),
        name: row.name,
        parentId: present(row.parent_id) ? uuid(row.parent_id) : null,
        revisionId: present(row.revision_id) ? uuid(row.revision_id) : null,
        size: present(row.size) ? row.size : null,
        contentHash: present(row.content_hash) ? hex(row.content_hash) : null,
        mediaType: present(row.media_type) ? row.media_type : null,
        deleted: row.deleted === true,
        end: row.end === true,
      });
    }),
    schemes.thumbState.on((row) =>
      take({ type: 'thumbState', contentHash: hex(row.content_hash), state: thumbOf(row.state) }),
    ),
    schemes.reset.on((row) =>
      take({ type: 'reset', reason: row.reason, epoch: row.epoch, seq: row.seq }),
    ),
    schemes.pong.on((row) => take({ type: 'pong', nonce: row.nonce })),
  ] as unknown as [Handler, ...Handler[]];
  const outcome = dispatch(message, handlers);
  if (!outcome.ok) return outcome;
  if (invalid !== null) return { ok: false, refusal: invalid };
  if (frame === null) {
    return { ok: false, refusal: { reason: 'malformed', type: 0, field: '', needed: 0, left: 0 } };
  }
  return { ok: true, frame };
}

/** The server-side view of a client message; the client itself only encodes these, tests decode them. */
export function decodeClientFrame(message: ArrayBuffer | Uint8Array): Decoded<ClientFrame> {
  let frame: ClientFrame | null = null;
  const handlers = [
    schemes.subscribe.on((row) => {
      frame = { type: 'subscribe', epoch: row.epoch, seq: row.seq };
    }),
    schemes.ping.on((row) => {
      frame = { type: 'ping', nonce: row.nonce };
    }),
  ] as unknown as [Handler, ...Handler[]];
  const outcome = dispatch(message, handlers);
  if (!outcome.ok) return outcome;
  if (frame === null) {
    return { ok: false, refusal: { reason: 'malformed', type: 0, field: '', needed: 0, left: 0 } };
  }
  return { ok: true, frame };
}

export function encodeSubscribe(epoch: bigint, seq: bigint): Uint8Array {
  return BinaryPacker.pack(schemes.subscribe, { epoch, seq });
}

export function encodePing(nonce: number): Uint8Array {
  return BinaryPacker.pack(schemes.ping, { nonce });
}
