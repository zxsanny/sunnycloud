import { api, type ApiError, type NodeView, type Result } from '$lib/api';
import { previewKind, type ThumbState } from './preview';

const PAGE_SIZE = 200;

export type FolderStatus = 'idle' | 'loading' | 'ready' | 'failed';

/** Folders first, then by name; the order the server pages in (by name key). */
function compare(a: NodeView, b: NodeView): number {
  if (a.kind !== b.kind) return a.kind === 'folder' ? -1 : 1;
  const left = a.name.toLowerCase();
  const right = b.name.toLowerCase();
  return left < right ? -1 : left > right ? 1 : 0;
}

/**
 * The open folder as the server lists it. Rows come from three places and merge by node id: the REST first
 * load and its pages, the answer of a create, and the frames pushed over the realtime channel (a Change is a
 * row, a ThumbState a thumbnail state). `upsert`, `remove` and `setThumbState` are the entry points for all of
 * them; the rule is "the higher feed position (`seq`) wins". No path is cached: every open reads the folder and
 * its ancestors again.
 */
class FolderStore {
  status = $state<FolderStatus>('idle');
  folder = $state<NodeView | null>(null);
  /** Root first, the open folder last. */
  trail = $state<NodeView[]>([]);
  items = $state<NodeView[]>([]);
  next = $state<string | null>(null);
  loadingMore = $state(false);
  moreFailed = $state(false);
  /** Thumbnail state by content hash. */
  thumbs = $state<Record<string, ThumbState>>({});

  private generation = 0;
  private openedId: string | null = null;
  private opening: Promise<void> = Promise.resolve();

  /** Opens a folder by id, or the account's root when `id` is null. */
  open(id: string | null): Promise<void> {
    const generation = (this.generation += 1);
    this.openedId = id;
    this.opening = this.load(id, generation);
    return this.opening;
  }

  private async load(id: string | null, generation: number) {
    this.status = 'loading';
    this.items = [];
    this.next = null;
    this.moreFailed = false;
    this.folder = null;
    this.trail = [];

    const node = await (id === null ? api.getNodesRoot() : api.getNode(id));
    if (generation !== this.generation) return;
    if (!node.ok) {
      this.status = 'failed';
      return;
    }
    const folder = node.value;
    const page = await api.getNodeChildren(folder.node_id, null, PAGE_SIZE);
    if (generation !== this.generation) return;
    if (!page.ok) {
      this.status = 'failed';
      return;
    }
    this.folder = folder;
    this.items = page.value.items.filter((item) => item.state === 'live');
    this.next = page.value.next;
    this.status = 'ready';
    void this.loadThumbStates(this.items);
    void this.loadTrail(folder, generation);
  }

  /**
   * Reads the first page of the open folder again, in place (no skeleton), and drops the later pages. The realtime
   * client calls it after Hello and after a Reset. A folder whose open failed is opened again. True when the
   * listing is current.
   */
  async reload(): Promise<boolean> {
    await this.opening;
    if (this.status === 'idle') return true;
    if (this.status !== 'ready' || !this.folder) {
      await this.open(this.openedId);
      return this.status === 'ready';
    }
    const generation = this.generation;
    const folder = this.folder;
    const page = await api.getNodeChildren(folder.node_id, null, PAGE_SIZE);
    if (generation !== this.generation) return true;
    if (!page.ok) return false;
    this.items = page.value.items.filter((item) => item.state === 'live');
    this.next = page.value.next;
    this.moreFailed = false;
    void this.loadThumbStates(this.items);
    return true;
  }

  /** The next page by the cursor the server gave. */
  async loadMore() {
    const folder = this.folder;
    if (!folder || this.next === null || this.loadingMore) return;
    const generation = this.generation;
    this.loadingMore = true;
    this.moreFailed = false;
    const page = await api.getNodeChildren(folder.node_id, this.next, PAGE_SIZE);
    if (generation !== this.generation) return;
    this.loadingMore = false;
    if (!page.ok) {
      this.moreFailed = true;
      return;
    }
    const known = new Set(this.items.map((item) => item.node_id));
    const fresh = page.value.items.filter(
      (item) => item.state === 'live' && !known.has(item.node_id),
    );
    this.items = [...this.items, ...fresh];
    this.next = page.value.next;
    void this.loadThumbStates(fresh);
  }

  /**
   * Adds or replaces a row of the open folder. A row with a lower `seq` than the one held is stale and ignored;
   * a tombstone removes the row; a new row that sorts after the last loaded one while more pages follow is left
   * for paging to deliver.
   */
  upsert(node: NodeView) {
    const folder = this.folder;
    if (!folder || node.parent_id !== folder.node_id) return;
    if (node.state === 'tombstone') {
      this.remove(node.node_id);
      return;
    }
    const at = this.items.findIndex((item) => item.node_id === node.node_id);
    const held = this.items[at];
    if (held) {
      if (node.seq < held.seq) return;
      this.items[at] = node;
    } else {
      const last = this.items[this.items.length - 1];
      if (this.next !== null && last && compare(node, last) > 0) return;
      const place = this.items.findIndex((item) => compare(node, item) < 0);
      this.items.splice(place === -1 ? this.items.length : place, 0, node);
    }
    this.expectPreview(node);
  }

  remove(nodeId: string) {
    this.items = this.items.filter((item) => item.node_id !== nodeId);
  }

  /** Seam for a pushed thumbnail-state frame (realtime, day 3); the REST first load uses it too. */
  setThumbState(contentHash: string, state: ThumbState) {
    this.thumbs[contentHash] = state;
  }

  async createFolder(name: string): Promise<Result<NodeView>> {
    const parent = this.folder;
    if (!parent) throw new Error('createFolder needs an open folder');
    const created = await api.postNode(parent.node_id, name);
    if (created.ok) this.upsert(created.value);
    return created;
  }

  async deleteNode(node: NodeView): Promise<Result<unknown, ApiError>> {
    const base = node.kind === 'file' ? node.current_revision : null;
    const result = await api.deleteNode(node.node_id, base);
    if (result.ok) this.remove(node.node_id);
    return result;
  }

  /** A file that just appeared has no thumbnail yet; its state arrives as a pushed frame. */
  private expectPreview(node: NodeView) {
    const hash = node.content_hash;
    if (hash && previewKind(node.media_type) !== 'none' && this.thumbs[hash] === undefined) {
      this.thumbs[hash] = 'pending';
    }
  }

  /**
   * One `POST /thumbnail-states` for the hashes of this folder whose preview is still pending: the realtime
   * client's catch-up after a reconnect (a ready flip during the gap is not replayed). Not a timer's call.
   */
  async refreshPendingThumbs() {
    const generation = this.generation;
    const hashes = [
      ...new Set(
        this.items
          .map((item) => item.content_hash)
          .filter(
            (hash): hash is string => typeof hash === 'string' && this.thumbs[hash] === 'pending',
          ),
      ),
    ];
    if (hashes.length === 0) return;
    const answer = await api.postThumbnailStates(hashes);
    if (generation !== this.generation || !answer.ok) return;
    for (const hash of hashes) {
      const state = answer.value.states[hash];
      if (state) this.thumbs[hash] = state;
    }
  }

  /** One `POST /thumbnail-states` for the previewable files of a page (a REST first load, not a poll). */
  private async loadThumbStates(rows: NodeView[]) {
    const generation = this.generation;
    const hashes = [
      ...new Set(
        rows
          .filter((row) => row.kind === 'file' && previewKind(row.media_type) !== 'none')
          .map((row) => row.content_hash)
          .filter(
            (hash): hash is string => typeof hash === 'string' && this.thumbs[hash] === undefined,
          ),
      ),
    ];
    if (hashes.length === 0) return;
    const answer = await api.postThumbnailStates(hashes);
    if (generation !== this.generation) return;
    for (const hash of hashes) {
      // A hash the server leaves out has no preview job: it shows the neutral icon.
      this.thumbs[hash] = answer.ok ? (answer.value.states[hash] ?? 'none') : 'none';
    }
  }

  private async loadTrail(folder: NodeView, generation: number) {
    const chain: NodeView[] = [folder];
    let parent = folder.parent_id;
    while (parent) {
      const node = await api.getNode(parent);
      if (generation !== this.generation) return;
      if (!node.ok) break;
      chain.unshift(node.value);
      parent = node.value.parent_id;
    }
    this.trail = chain;
  }
}

export const folderStore = new FolderStore();
