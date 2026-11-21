import { BinaryPacker } from 'packbin';
import { schemes } from '../../src/lib/realtime/frames';

const raw = (uuid: string | null) =>
  uuid === null
    ? null
    : Uint8Array.from(uuid.replaceAll('-', '').match(/../g) ?? [], (b) => parseInt(b, 16));
const hashBytes = (hex: string) => Uint8Array.from(hex.match(/../g) ?? [], (b) => parseInt(b, 16));

export interface ChangeSpec {
  seq: number;
  nodeId: string;
  parentId: string | null;
  name: string;
  kind?: 'file' | 'folder';
  thumb?: 0 | 1 | 2 | 3;
  contentHash?: string | null;
  mediaType?: string | null;
  size?: number | null;
  deleted?: boolean;
  end?: boolean;
}

/** Packs a Change frame the way the server does (packbin is the reference on both sides). */
export function changeBytes(spec: ChangeSpec): Uint8Array {
  return BinaryPacker.pack(schemes.change, {
    epoch: 1n,
    seq: BigInt(spec.seq),
    node_id: raw(spec.nodeId) as Uint8Array,
    kind: spec.kind === 'folder' ? 1 : 0,
    thumb: spec.thumb ?? 0,
    name: spec.name,
    parent_id: raw(spec.parentId),
    revision_id: spec.deleted ? null : raw('0190cccc-0000-7000-8000-000000000001'),
    size: spec.size === undefined || spec.size === null ? null : BigInt(spec.size),
    content_hash: spec.contentHash ? hashBytes(spec.contentHash) : null,
    media_type: spec.mediaType ?? null,
    deleted: spec.deleted ? true : null,
    end: spec.end === false ? null : true,
  });
}

export function thumbStateBytes(contentHash: string, state: 2 | 3): Uint8Array {
  return BinaryPacker.pack(schemes.thumbState, { content_hash: hashBytes(contentHash), state });
}

export const helloBytes = (intervalS = 30) =>
  BinaryPacker.pack(schemes.hello, {
    version: 1,
    ping_interval_s: intervalS,
    max_client_frame: 1024,
  });

export const resetBytes = (reason: number, epoch: bigint, seq: bigint) =>
  BinaryPacker.pack(schemes.reset, { reason, epoch, seq });
