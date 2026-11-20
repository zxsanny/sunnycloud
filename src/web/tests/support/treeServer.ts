import { account, json, problem } from './http';

export interface Node {
  node_id: string;
  parent_id: string | null;
  kind: 'file' | 'folder';
  name: string;
  state: 'live' | 'tombstone';
  current_revision: string | null;
  size: number | null;
  content_hash: string | null;
  media_type: string | null;
  client_mtime: string | null;
  created_at: string;
  updated_at: string;
  seq: number;
}

let counter = 0;
export const uuid = () => `0190bbbb-0000-7000-8000-${String((counter += 1)).padStart(12, '0')}`;

export function makeNode(partial: Partial<Node> & Pick<Node, 'name' | 'kind'>): Node {
  const id = partial.node_id ?? uuid();
  return {
    node_id: id,
    parent_id: null,
    state: 'live',
    current_revision: partial.kind === 'file' ? uuid() : null,
    size: partial.kind === 'file' ? 1234 : null,
    content_hash: partial.kind === 'file' ? id.replace(/-/g, '').padEnd(64, 'a') : null,
    media_type: partial.kind === 'file' ? 'application/octet-stream' : null,
    client_mtime: null,
    created_at: '2026-10-30T10:00:00Z',
    updated_at: '2026-10-30T10:00:00Z',
    seq: 1,
    ...partial,
  };
}

/** A fake of the tree and thumbnail routes: folders first, name order, cursor paging. */
export class TreeServer {
  readonly nodes = new Map<string, Node>();
  readonly requests: { method: string; path: string; query: URLSearchParams; body?: unknown }[] =
    [];
  readonly root: Node;
  /** Answer of `POST /thumbnail-states`, by content hash; a hash not listed is left out of the answer. */
  thumbStates: Record<string, string> = {};
  /** Delays or fails a route: return a Response (or a promise of one) to answer instead. */
  intercept: ((method: string, path: string) => Response | Promise<Response> | undefined) | null =
    null;

  constructor() {
    this.root = makeNode({ name: '', kind: 'folder', parent_id: null });
    this.nodes.set(this.root.node_id, this.root);
  }

  add(parent: Node, partial: Partial<Node> & Pick<Node, 'name' | 'kind'>): Node {
    const node = makeNode({ ...partial, parent_id: parent.node_id });
    this.nodes.set(node.node_id, node);
    return node;
  }

  childrenOf(id: string): Node[] {
    return [...this.nodes.values()]
      .filter((n) => n.parent_id === id && n.state === 'live')
      .sort((a, b) =>
        a.kind !== b.kind
          ? a.kind === 'folder'
            ? -1
            : 1
          : a.name.toLowerCase().localeCompare(b.name.toLowerCase()),
      );
  }

  readonly fetch = async (request: Request): Promise<Response> => {
    const url = new URL(request.url);
    const text = await request.clone().text();
    const body = text === '' ? undefined : JSON.parse(text);
    this.requests.push({
      method: request.method,
      path: url.pathname,
      query: url.searchParams,
      body,
    });
    const forced = this.intercept?.(request.method, url.pathname);
    if (forced) return forced;
    const path = url.pathname;
    if (request.method === 'GET' && path === '/api/v1/accounts/me') return json(account);
    if (request.method === 'GET' && path === '/api/v1/nodes/root') return json(this.root);
    const children = /^\/api\/v1\/nodes\/([^/]+)\/children$/.exec(path);
    if (request.method === 'GET' && children)
      return this.listing(children[1] ?? '', url.searchParams);
    const one = /^\/api\/v1\/nodes\/([^/]+)$/.exec(path);
    if (one) return this.node(request.method, one[1] ?? '', url.searchParams);
    if (request.method === 'POST' && path === '/api/v1/nodes') return this.create(body);
    if (request.method === 'POST' && path === '/api/v1/thumbnail-states') {
      const states: Record<string, string> = {};
      for (const hash of (body as { content_hashes: string[] }).content_hashes) {
        const state = this.thumbStates[hash];
        if (state) states[hash] = state;
      }
      return json({ states });
    }
    throw new Error(`unplanned request: ${request.method} ${path}`);
  };

  private listing(id: string, query: URLSearchParams): Response {
    if (!this.nodes.has(id)) return problem(404, 'not_found');
    const all = this.childrenOf(id);
    const start = query.get('after') ? Number(query.get('after')?.slice(2)) : 0;
    const limit = Number(query.get('limit') ?? 1000);
    const items = all.slice(start, start + limit);
    const end = start + limit;
    return json({ items, next: end < all.length ? `c:${end}` : null });
  }

  private node(method: string, id: string, query: URLSearchParams): Response {
    const node = this.nodes.get(id);
    if (!node) return problem(404, 'not_found');
    if (method === 'GET') return json(node);
    if (method === 'DELETE') {
      if (node.kind === 'file' && query.get('base_revision') !== node.current_revision) {
        return problem(409, 'edited_since');
      }
      node.state = 'tombstone';
      return new Response(null, { status: 204 });
    }
    throw new Error(`unplanned request: ${method} node`);
  }

  private create(body: unknown): Response {
    const { parent_id, name } = body as { parent_id: string; name: string };
    if (this.childrenOf(parent_id).some((n) => n.name === name))
      return problem(409, 'name_conflict');
    if (name.includes('/')) return problem(422, 'invalid_name');
    const parent = this.nodes.get(parent_id);
    if (!parent) return problem(404, 'parent_not_found');
    return json(this.add(parent, { name, kind: 'folder' }), 201);
  }
}
