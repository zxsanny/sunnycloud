import { json, problem } from './http';
import { hex, wasm } from './wasm';

interface Session {
  id: string;
  clientKey: string;
  size: number;
  parentId: string;
  name: string;
  blocks: Map<number, { hash: string; length: number; body?: Uint8Array }>;
}

export interface Committed {
  nodeId: string;
  name: string;
  parentId: string;
  size: number;
  contentHash: string;
}

export interface ServerOptions {
  batchMaxEntries?: number;
  /** What server-info says about the staging cap, when it differs from the real cap (another tab holds some). */
  advertisedStagedMax?: number;
  maxStagedPerAccount?: number;
  maxLanes?: number;
  keepBodies?: boolean;
  /** Milliseconds a block PUT takes, so lanes overlap. */
  blockDelayMs?: number;
}

export type Fault = (request: Request, route: string) => Response | Promise<Response> | undefined;

/**
 * A fake server for the upload routes. It checks what the real one checks: `X-Blake3` against the body,
 * the blocklist against the received blocks, a repeated commit ID against its stored answer.
 */
export class UploadServer {
  readonly staged = new Map<string, { hash: string; body: Uint8Array }>();
  readonly sessions = new Map<string, Session>();
  readonly nodes: Committed[] = [];
  /** Content of published nodes by node id (only with `keepBodies`). */
  readonly contents = new Map<string, Uint8Array>();
  readonly requests: string[] = [];
  readonly batches: { commitId: string; entries: number }[] = [];
  readonly deleted: string[] = [];
  /** Every block the server accepted: its index, exact length and the X-Blake3 it carried. */
  readonly blockLog: { index: number; length: number; hash: string }[] = [];
  maxConcurrentBlocks = 0;
  private concurrentBlocks = 0;
  private readonly receipts = new Map<string, unknown>();
  private counter = 0;
  faults: Fault[] = [];

  constructor(readonly options: ServerOptions = {}) {}

  readonly fetch = async (request: Request): Promise<Response> => {
    const route = this.routeOf(request);
    this.requests.push(route);
    for (const fault of this.faults) {
      const answer = fault(request, route);
      if (answer !== undefined) return answer;
    }
    return this.route(request, new URL(request.url), route);
  };

  /** What the server would answer, skipping the faults (a fault that only delays a request calls this). */
  readonly bypass = (request: Request): Promise<Response> =>
    this.route(request, new URL(request.url), this.routeOf(request));

  private routeOf(request: Request): string {
    const path = new URL(request.url).pathname;
    return `${request.method} ${path.replace(/[0-9a-f]{8}-[0-9a-f-]{27}/g, ':id').replace(/\/blocks\/\d+/, '/blocks/:n')}`;
  }

  private id(): string {
    this.counter += 1;
    return `0190aaaa-0000-7000-8000-${String(this.counter).padStart(12, '0')}`;
  }

  private async route(request: Request, url: URL, route: string): Promise<Response> {
    const lastId = url.pathname.split('/').find((part) => /^[0-9a-f]{8}-/.test(part)) ?? '';
    switch (route) {
      case 'GET /api/v1/server-info':
        return json({
          block_size: 2 * 1024 * 1024,
          max_lanes: this.options.maxLanes ?? 4,
          small_file_max: 2 * 1024 * 1024,
          batch_max_entries: this.options.batchMaxEntries ?? 100,
          max_staged_per_account:
            this.options.advertisedStagedMax ?? this.options.maxStagedPerAccount ?? 1000,
          longpoll_max_wait: 25,
          min_client_version: '0.0.0',
          api_versions: ['v1'],
        });
      case 'PUT /api/v1/staged-uploads/:id':
        return this.stage(request, lastId);
      case 'POST /api/v1/commits:batch':
        return this.commitBatch(await request.json());
      case 'POST /api/v1/uploads':
        return this.start(await request.json());
      case 'GET /api/v1/uploads/:id':
        return this.session(lastId);
      case 'DELETE /api/v1/uploads/:id':
        this.deleted.push(lastId);
        return new Response(null, { status: 204 });
      case 'PUT /api/v1/uploads/:id/blocks/:n':
        return this.block(request, lastId, Number(url.pathname.split('/').pop()));
      case 'POST /api/v1/commits':
        return this.commit(await request.json());
      default:
        throw new Error(`unplanned request: ${route}`);
    }
  }

  private async stage(request: Request, key: string): Promise<Response> {
    const body = new Uint8Array(await request.arrayBuffer());
    const declared = request.headers.get('X-Blake3');
    const actual = hex(wasm.hash_block(body));
    if (declared !== actual) return problem(422, 'block_hash_mismatch');
    if (body.length > 2 * 1024 * 1024) return problem(413, 'size_too_large');
    const cap = this.options.maxStagedPerAccount ?? 1000;
    if (!this.staged.has(key) && this.staged.size >= cap) return problem(409, 'staged_cap_reached');
    this.staged.set(key, { hash: actual, body });
    return json({ staged_key: key, size: body.length });
  }

  private commitBatch(body: {
    commit_id: string;
    entries: { staged_key: string; parent_id: string; name: string }[];
  }): Response {
    this.batches.push({ commitId: body.commit_id, entries: body.entries.length });
    const stored = this.receipts.get(body.commit_id);
    if (stored) return json(stored);
    if (body.entries.length > (this.options.batchMaxEntries ?? 100))
      return problem(422, 'batch_too_large');
    const entries = body.entries.map((entry) => {
      const staged = this.staged.get(entry.staged_key);
      if (!staged) return { status: 'entry_error', code: 'not_found' };
      if (entry.name.includes('<bad>')) return { status: 'entry_error', code: 'invalid_name' };
      this.staged.delete(entry.staged_key);
      const node = {
        nodeId: this.id(),
        name: entry.name,
        parentId: entry.parent_id,
        size: staged.body.length,
        contentHash: hex(wasm.rollup(wasm.hash_block(staged.body))),
      };
      this.nodes.push(node);
      return { status: 'committed', node_id: node.nodeId, revision_id: this.id() };
    });
    const answer = { entries };
    this.receipts.set(body.commit_id, answer);
    return json(answer);
  }

  private start(body: {
    parent_id: string;
    name: string;
    declared_size: number;
    client_key: string;
    content_hash?: string;
  }): Response {
    const existing = this.nodes.find(
      (n) =>
        n.name === body.name &&
        n.parentId === body.parent_id &&
        n.contentHash === body.content_hash,
    );
    if (existing) return json({ outcome: 'nothing_to_send', revision_id: this.id() });
    let session = [...this.sessions.values()].find((s) => s.clientKey === body.client_key);
    if (!session) {
      session = {
        id: this.id(),
        clientKey: body.client_key,
        size: body.declared_size,
        parentId: body.parent_id,
        name: body.name,
        blocks: new Map(),
      };
      this.sessions.set(session.id, session);
    }
    return json({
      outcome: 'session',
      upload_id: session.id,
      need_blocks: this.needed(session),
      block_size: 2 * 1024 * 1024,
      lane_limit: this.options.maxLanes ?? 4,
      expires_at: '2030-01-01T00:00:00Z',
    });
  }

  private needed(session: Session): number[] {
    const count = Math.ceil(session.size / (2 * 1024 * 1024));
    return Array.from({ length: count }, (_, i) => i).filter((i) => !session.blocks.has(i));
  }

  private session(id: string): Response {
    const session = this.sessions.get(id);
    if (!session) return problem(410, 'upload_session_gone');
    const need = this.needed(session);
    return json({
      upload_id: id,
      need_blocks: need,
      received_count: session.blocks.size,
      expires_at: '2030-01-01T00:00:00Z',
    });
  }

  private async block(request: Request, id: string, index: number): Promise<Response> {
    const session = this.sessions.get(id);
    if (!session) return problem(410, 'upload_session_gone');
    this.concurrentBlocks += 1;
    this.maxConcurrentBlocks = Math.max(this.maxConcurrentBlocks, this.concurrentBlocks);
    try {
      const body = new Uint8Array(await request.arrayBuffer());
      if (this.options.blockDelayMs) {
        await new Promise((resolve) => setTimeout(resolve, this.options.blockDelayMs));
        if (request.signal.aborted) throw new DOMException('aborted', 'AbortError');
      }
      const hash = hex(wasm.hash_block(body));
      if (hash !== request.headers.get('X-Blake3')) return problem(422, 'block_hash_mismatch');
      this.blockLog.push({ index, length: body.length, hash });
      session.blocks.set(index, {
        hash,
        length: body.length,
        body: this.options.keepBodies ? body : undefined,
      });
      return new Response(null, { status: 204 });
    } finally {
      this.concurrentBlocks -= 1;
    }
  }

  private commit(body: {
    upload_id: string;
    parent_id: string;
    name: string;
    blocklist: string[];
  }): Response {
    const session = this.sessions.get(body.upload_id);
    if (!session) return problem(410, 'upload_session_gone');
    const need = this.needed(session);
    if (need.length > 0) return problem(409, 'need_blocks', { need_blocks: need });
    const received = [...session.blocks.entries()]
      .sort((a, b) => a[0] - b[0])
      .map(([, b]) => b.hash);
    if (received.join() !== body.blocklist.join()) return problem(422, 'blocklist_mismatch');
    const hashes = new Uint8Array(received.length * 32);
    received.forEach((h, i) =>
      hashes.set(
        Uint8Array.from(h.match(/../g) ?? [], (x) => parseInt(x, 16)),
        i * 32,
      ),
    );
    const node = {
      nodeId: this.id(),
      name: body.name,
      parentId: body.parent_id,
      size: session.size,
      contentHash: hex(wasm.rollup(hashes)),
    };
    this.nodes.push(node);
    if (this.options.keepBodies) {
      const parts = [...session.blocks.entries()]
        .sort((a, b) => a[0] - b[0])
        .map(([, b]) => b.body ?? new Uint8Array());
      this.contents.set(node.nodeId, new Uint8Array(Buffer.concat(parts)));
    }
    this.sessions.delete(body.upload_id);
    return json({ outcome: 'published', node_id: node.nodeId, revision_id: this.id() });
  }
}
