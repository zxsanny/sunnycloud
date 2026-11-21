import { json, problem } from './http';
import { patternBytes } from './files';
import { hex, wasm } from './wasm';

/** The server's file hash of `bytes`: the roll-up of the 2 MiB block hashes, from the one implementation (sync-core-wasm). */
export function fileHashOf(bytes: Uint8Array): string {
  const block = wasm.block_size();
  const hashes: number[] = [];
  for (let at = 0; at < bytes.length; at += block) {
    hashes.push(...wasm.hash_block(bytes.subarray(at, at + block)));
  }
  return hex(wasm.rollup(Uint8Array.from(hashes)));
}

export interface StoredFile {
  name: string;
  bytes: Uint8Array;
  state?: 'live' | 'tombstone';
  /** What the metadata says; the default is the real hash of `bytes`. */
  hash?: string;
  /** The size the metadata says, for a file too big to build (a path that reads no byte needs no bytes). */
  declaredSize?: number;
}

export interface ContentFault {
  /** Flip one bit of the byte at this file offset in every answer that carries it. */
  flipAt?: number;
  /** End the body after this many bytes of an answer with an error, as a dropped connection does. */
  cutAfter?: number;
  /** Answer 200 (the whole file) to a ranged request. */
  ignoreRange?: boolean;
  /** Never answer a content request. */
  hang?: boolean;
}

/** A fake of the download routes: metadata, ranged content in small chunks, revisions that only admins may read. */
export class ContentServer {
  readonly files = new Map<string, StoredFile>();
  readonly requests: { route: string; range: string | null }[] = [];
  fault: ContentFault = {};
  /** Share token to file. */
  readonly shares = new Map<string, StoredFile>();
  /** Revision ids a non-admin may not read. */
  readonly retired = new Set<string>();
  chunk = 300_000;

  add(id: string, size: number, name = `${id}.bin`): StoredFile {
    const file: StoredFile = { name, bytes: patternBytes(size) };
    this.files.set(id, file);
    return file;
  }

  readonly fetch = async (request: Request): Promise<Response> => {
    const url = new URL(request.url);
    const range = request.headers.get('Range');
    const route = request.method + ' ' + url.pathname.replace(/[0-9a-f]{8}-[0-9a-f-]{27}/g, ':id');
    this.requests.push({ route, range });
    const [, , , kind, id, tail] = url.pathname.split('/');
    if (kind === 'nodes' && tail === undefined) return this.node(id as string);
    if (kind === 'nodes' && tail === 'content') {
      if (this.fault.hang === true) return new Promise<Response>(() => {});
      return this.content(id as string, range);
    }
    if (kind === 'revisions' && tail === undefined) return this.revision(id as string);
    if (kind === 'revisions' && tail === 'content') return this.content(id as string, range);
    if (kind === 'public-shares' && tail === undefined) return this.share(id as string);
    throw new Error(`unplanned request: ${route}`);
  };

  private node(id: string): Response {
    const file = this.files.get(id);
    if (!file) return problem(404, 'not_found');
    return json({
      node_id: id,
      kind: 'file',
      name: file.name,
      state: file.state ?? 'live',
      size: file.declaredSize ?? file.bytes.length,
      content_hash: file.hash ?? fileHashOf(file.bytes),
      current_revision: `rev-${id}`,
      created_at: '2026-11-21T10:00:00Z',
      updated_at: '2026-11-21T10:00:00Z',
      seq: 1,
    });
  }

  private revision(id: string): Response {
    if (this.retired.has(id)) return problem(403, 'forbidden');
    const file = this.files.get(id.replace(/^rev-/, '')) ?? this.files.get(id);
    if (!file) return problem(404, 'not_found');
    return json({
      revision_id: id,
      node_id: id.replace(/^rev-/, ''),
      size: file.bytes.length,
      content_hash: file.hash ?? fileHashOf(file.bytes),
      created_at: '2026-11-21T10:00:00Z',
    });
  }

  private share(token: string): Response {
    const file = this.shares.get(token);
    if (!file) return problem(404, 'not_found');
    return json({
      name: file.name,
      size: file.bytes.length,
      media_type: null,
      expires_at: '2026-12-01T00:00:00Z',
    });
  }

  private content(id: string, range: string | null): Response {
    if (this.retired.has(id)) return problem(404, 'not_found');
    const file = this.files.get(id.replace(/^rev-/, ''));
    if (!file || file.state === 'tombstone') return problem(404, 'not_found');
    const match = /^bytes=(\d+)-(\d+)$/.exec(range ?? '');
    const whole = match === null || this.fault.ignoreRange === true;
    const last = file.bytes.length - 1;
    const start = match === null || whole ? 0 : Number(match[1]);
    const end = match === null || whole ? last : Math.min(Number(match[2]), last);
    const bytes = file.bytes.slice(start, end + 1);
    const flip = this.fault.flipAt;
    if (flip !== undefined && flip >= start && flip <= end) {
      bytes[flip - start] = (bytes[flip - start] ?? 0) ^ 1;
    }
    const cut = this.fault.cutAfter;
    let sent = 0;
    const body = new ReadableStream<Uint8Array>({
      pull: (controller) => {
        if (cut !== undefined && sent >= cut) {
          controller.error(new TypeError('network error'));
          return;
        }
        if (sent >= bytes.length) {
          controller.close();
          return;
        }
        const next = Math.min(this.chunk, bytes.length - sent);
        controller.enqueue(bytes.slice(sent, sent + next));
        sent += next;
      },
    });
    return new Response(body, {
      status: whole ? 200 : 206,
      headers: whole ? {} : { 'Content-Range': `bytes ${start}-${end}/${file.bytes.length}` },
    });
  }
}
