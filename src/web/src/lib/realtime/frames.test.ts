import { BinaryPacker } from 'packbin';
import { describe, expect, it } from 'vitest';
import { hexOf, loadVectors } from '../../../tests/support/realtimeVectors';
import {
  decodeClientFrame,
  decodeServerFrame,
  encodePing,
  encodeSubscribe,
  schemes,
  type ChangeFrame,
} from './frames';

const vectors = loadVectors();
const vector = (name: string) => {
  const found = vectors.get(name);
  if (!found) throw new Error(`no vector ${name}`);
  return found;
};

const seq = (from: number, count: number) => Uint8Array.from({ length: count }, (_, i) => from + i);
const hexSeq = (from: number, count: number) => hexOf(seq(from, count));

/** The row packbin packs for a decoded server frame, to prove fields and bytes agree both ways. */
function repack(frame: ReturnType<typeof expectFrame>): Uint8Array {
  const raw = (hex: string | null) =>
    hex === null
      ? null
      : Uint8Array.from(hex.replaceAll('-', '').match(/../g) ?? [], (b) => parseInt(b, 16));
  switch (frame.type) {
    case 'hello':
      return BinaryPacker.pack(schemes.hello, {
        version: frame.version,
        ping_interval_s: frame.pingIntervalS,
        max_client_frame: frame.maxClientFrame,
      });
    case 'pong':
      return BinaryPacker.pack(schemes.pong, { nonce: frame.nonce });
    case 'reset':
      return BinaryPacker.pack(schemes.reset, {
        reason: frame.reason,
        epoch: frame.epoch,
        seq: frame.seq,
      });
    case 'thumbState':
      return BinaryPacker.pack(schemes.thumbState, {
        content_hash: raw(frame.contentHash) as Uint8Array,
        state: ['none', 'pending', 'ready', 'failed'].indexOf(frame.state),
      });
    case 'change':
      return BinaryPacker.pack(schemes.change, {
        epoch: frame.epoch,
        seq: frame.seq,
        node_id: raw(frame.nodeId) as Uint8Array,
        kind: frame.nodeKind === 'file' ? 0 : 1,
        thumb: ['none', 'pending', 'ready', 'failed'].indexOf(frame.thumb),
        name: frame.name,
        parent_id: raw(frame.parentId),
        revision_id: raw(frame.revisionId),
        size: frame.size,
        content_hash: raw(frame.contentHash),
        media_type: frame.mediaType,
        deleted: frame.deleted ? true : null,
        end: frame.end ? true : null,
      });
  }
}

function expectFrame(bytes: Uint8Array) {
  const decoded = decodeServerFrame(bytes);
  if (!decoded.ok) throw new Error(`refused: ${JSON.stringify(decoded.refusal)}`);
  return decoded.frame;
}

describe('the client frames', () => {
  it('encodes Subscribe epoch 1 seq 42 to the bytes of the shared vector', () => {
    // Act
    const bytes = encodeSubscribe(1n, 42n);

    // Assert
    expect(hexOf(bytes)).toBe(hexOf(vector('subscribe')));
  });

  it('encodes Ping nonce 7 to the bytes of the shared vector', () => {
    // Act
    const bytes = encodePing(7);

    // Assert
    expect(hexOf(bytes)).toBe(hexOf(vector('ping')));
  });

  it('decodes the client vectors to their fields and encodes them back byte for byte', () => {
    // Act
    const subscribe = decodeClientFrame(vector('subscribe'));
    const ping = decodeClientFrame(vector('ping'));

    // Assert
    expect(subscribe).toEqual({ ok: true, frame: { type: 'subscribe', epoch: 1n, seq: 42n } });
    expect(ping).toEqual({ ok: true, frame: { type: 'ping', nonce: 7 } });
    expect(hexOf(encodeSubscribe(1n, 42n))).toBe(hexOf(vector('subscribe')));
  });

  it('carries the full 64-bit range of a resume point', () => {
    // Act
    const bytes = encodeSubscribe(2n ** 64n - 1n, 2n ** 63n);

    // Assert
    expect(decodeClientFrame(bytes)).toEqual({
      ok: true,
      frame: { type: 'subscribe', epoch: 2n ** 64n - 1n, seq: 2n ** 63n },
    });
  });
});

describe('the server frames: the good vectors', () => {
  it('decodes Hello', () => {
    // Act
    const frame = expectFrame(vector('hello'));

    // Assert
    expect(frame).toEqual({ type: 'hello', version: 1, pingIntervalS: 30, maxClientFrame: 1024 });
  });

  it('decodes Pong', () => {
    // Act
    const frame = expectFrame(vector('pong'));

    // Assert
    expect(frame).toEqual({ type: 'pong', nonce: 7 });
  });

  it('decodes ThumbState with its content hash', () => {
    // Act
    const frame = expectFrame(vector('thumb_state_ready'));

    // Assert
    expect(frame).toEqual({ type: 'thumbState', contentHash: hexSeq(0x20, 32), state: 'ready' });
  });

  it('decodes Reset with its reason and the head', () => {
    // Act
    const frame = expectFrame(vector('reset_below_horizon'));

    // Assert
    expect(frame).toEqual({ type: 'reset', reason: 2, epoch: 1n, seq: 900n });
  });

  it('decodes a live file Change with every optional member and the end flag', () => {
    // Act
    const frame = expectFrame(vector('change_file'));

    // Assert
    expect(frame).toEqual({
      type: 'change',
      epoch: 1n,
      seq: 42n,
      nodeId: '01020304-0506-0708-090a-0b0c0d0e0f10',
      nodeKind: 'file',
      thumb: 'pending',
      name: 'IMG_0001.HEIC',
      parentId: '30313233-3435-3637-3839-3a3b3c3d3e3f',
      revisionId: '50515253-5455-5657-5859-5a5b5c5d5e5f',
      size: 2345678n,
      contentHash: hexSeq(0x20, 32),
      mediaType: 'image/heic',
      deleted: false,
      end: true,
    } satisfies ChangeFrame);
  });

  it('decodes a tombstone folder with a Cyrillic name', () => {
    // Act
    const frame = expectFrame(vector('change_folder_tombstone'));

    // Assert
    expect(frame).toMatchObject({
      type: 'change',
      seq: 43n,
      nodeKind: 'folder',
      thumb: 'none',
      name: 'Старі фото',
      parentId: '30313233-3435-3637-3839-3a3b3c3d3e3f',
      deleted: true,
      end: false,
      revisionId: null,
      size: null,
      contentHash: null,
      mediaType: null,
    });
  });

  it('decodes a minimal folder with no optional member', () => {
    // Act
    const frame = expectFrame(vector('change_root_child_minimal'));

    // Assert
    expect(frame).toEqual({
      type: 'change',
      epoch: 1n,
      seq: 44n,
      nodeId: '03040506-0708-090a-0b0c-0d0e0f101112',
      nodeKind: 'folder',
      thumb: 'none',
      name: 'a',
      parentId: null,
      revisionId: null,
      size: null,
      contentHash: null,
      mediaType: null,
      deleted: false,
      end: false,
    });
  });

  it.each([
    'hello',
    'pong',
    'thumb_state_ready',
    'reset_below_horizon',
    'change_file',
    'change_folder_tombstone',
    'change_root_child_minimal',
  ])('packs the decoded fields of %s back to the bytes of the vector', (name) => {
    // Act
    const bytes = repack(expectFrame(vector(name)));

    // Assert
    expect(hexOf(bytes)).toBe(hexOf(vector(name)));
  });

  it('accepts a Change whose unused flag bit 7 is set (receivers ignore it)', () => {
    // Arrange
    const bytes = Uint8Array.from(vector('change_root_child_minimal'));
    bytes[bytes.length - 1] = 0x80;

    // Act
    const decoded = decodeServerFrame(bytes);

    // Assert
    expect(decoded.ok).toBe(true);
  });

  it('treats an unknown thumb code as none', () => {
    // Arrange
    const bytes = Uint8Array.from(vector('change_root_child_minimal'));
    // node id is 16 bytes after type, epoch and seq; kind then thumb
    bytes[1 + 8 + 8 + 16 + 1] = 9;

    // Act
    const decoded = decodeServerFrame(bytes);

    // Assert
    expect(decoded).toMatchObject({ ok: true, frame: { thumb: 'none' } });
  });
});

describe('the server frames: hostile input is refused, never thrown', () => {
  it.each([
    'bad_server_hello_truncated',
    'bad_server_hello_trailing_byte',
    'bad_server_change_name_length_lies',
  ])('refuses %s', (name) => {
    // Act
    const decoded = decodeServerFrame(vector(name));

    // Assert
    expect(decoded.ok).toBe(false);
    if (!decoded.ok) expect(decoded.refusal.reason).toBe('malformed');
  });

  it('refuses the empty message', () => {
    // Act
    const decoded = decodeServerFrame(vector('bad_client_empty'));

    // Assert
    expect(decoded).toEqual({ ok: false, refusal: { reason: 'empty' } });
  });

  it('refuses an unknown first byte and names it', () => {
    // Act
    const decoded = decodeServerFrame(vector('bad_client_unknown_type'));

    // Assert
    expect(decoded).toEqual({ ok: false, refusal: { reason: 'unknown_type', type: 0x7e } });
  });

  it('refuses a client frame number that arrives from the server', () => {
    // Act
    const decoded = decodeServerFrame(vector('subscribe'));

    // Assert
    expect(decoded).toEqual({ ok: false, refusal: { reason: 'unknown_type', type: 0x01 } });
  });

  it.each(['change_file', 'change_folder_tombstone', 'reset_below_horizon', 'thumb_state_ready'])(
    'refuses every truncation of %s',
    (name) => {
      // Arrange
      const full = vector(name);

      // Act
      const results = Array.from({ length: full.length - 1 }, (_, n) =>
        decodeServerFrame(full.subarray(0, n + 1)),
      );

      // Assert
      expect(results.every((r) => !r.ok)).toBe(true);
    },
  );

  it.each(['change_file', 'hello', 'pong', 'reset_below_horizon', 'thumb_state_ready'])(
    'refuses a trailing byte after %s',
    (name) => {
      // Arrange
      const full = vector(name);
      const padded = new Uint8Array(full.length + 1);
      padded.set(full);

      // Act
      const decoded = decodeServerFrame(padded);

      // Assert
      expect(decoded.ok).toBe(false);
    },
  );

  it('refuses a Change with a kind that is neither file nor folder', () => {
    // Arrange
    const bytes = Uint8Array.from(vector('change_root_child_minimal'));
    bytes[1 + 8 + 8 + 16] = 5;

    // Act
    const decoded = decodeServerFrame(bytes);

    // Assert
    expect(decoded).toEqual({
      ok: false,
      refusal: { reason: 'invalid_value', type: 0x21, field: 'kind' },
    });
  });

  it('refuses invalid compact text in a name without throwing', () => {
    // Arrange: name length 255 with nothing behind it
    const bytes = Uint8Array.from(
      vector('change_root_child_minimal').subarray(0, 1 + 8 + 8 + 16 + 2),
    );
    const lying = new Uint8Array([...bytes, 0xff, 0x01]);

    // Act
    const decoded = decodeServerFrame(lying);

    // Assert
    expect(decoded.ok).toBe(false);
  });

  it('never throws on random bytes', () => {
    // Arrange
    let state = 12345;
    const next = () => (state = (state * 1103515245 + 12345) & 0x7fffffff) & 0xff;

    // Act
    const throwing = Array.from({ length: 2000 }, () => {
      const bytes = Uint8Array.from({ length: next() % 80 }, next);
      if (bytes.length > 0) bytes[0] = [0x20, 0x21, 0x22, 0x23, 0x24, next()][next() % 6] ?? 0x21;
      try {
        decodeServerFrame(bytes);
        return false;
      } catch {
        return true;
      }
    }).filter(Boolean);

    // Assert
    expect(throwing).toHaveLength(0);
  });
});

describe('the client frames: hostile input is refused (what the server must refuse, from the shared file)', () => {
  it.each([
    'bad_client_empty',
    'bad_client_unknown_type',
    'bad_client_server_only_type',
    'bad_client_subscribe_truncated',
    'bad_client_subscribe_trailing_byte',
    'bad_client_ping_trailing_byte',
  ])('refuses %s', (name) => {
    // Act
    const decoded = decodeClientFrame(vector(name));

    // Assert
    expect(decoded.ok).toBe(false);
  });
});
