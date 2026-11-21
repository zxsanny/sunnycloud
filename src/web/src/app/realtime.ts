import { api, type ApiError, type NodeView } from '$lib/api';
import { folderStore } from '$lib/files';
import { notices } from '$lib/notices';
import {
  RealtimeClient,
  parseCursor,
  realtimeUrl,
  type ChangeFrame,
  type RealtimeOptions,
} from '$lib/realtime';

/**
 * A Change frame is the latest state of one node. It lands in the open folder when the node's parent is that folder;
 * a tombstone, or a node that moved to another folder, leaves it. The thumbnail state it carries goes to the same
 * place as a ThumbState frame.
 */
export function applyChange(frame: ChangeFrame) {
  const folder = folderStore.folder;
  if (frame.contentHash !== null) folderStore.setThumbState(frame.contentHash, frame.thumb);
  if (folder === null) return;
  if (frame.deleted || frame.parentId !== folder.node_id) {
    folderStore.remove(frame.nodeId);
    return;
  }
  const held = folderStore.items.find((item) => item.node_id === frame.nodeId);
  const node: NodeView = {
    node_id: frame.nodeId,
    parent_id: frame.parentId,
    kind: frame.nodeKind,
    name: frame.name,
    state: 'live',
    current_revision: frame.revisionId,
    size: frame.size === null ? null : Number(frame.size),
    content_hash: frame.contentHash,
    media_type: frame.mediaType,
    // The frame carries no timestamps: keep the ones the listing gave, or show none.
    client_mtime: held?.client_mtime ?? null,
    created_at: held?.created_at ?? '',
    updated_at: held?.updated_at ?? '',
    seq: Number(frame.seq),
  };
  folderStore.upsert(node);
}

const SESSION_ENDED: ApiError = {
  kind: 'http',
  status: 401,
  code: 'unauthenticated',
  title: null,
  requestId: null,
  retryAfterSeconds: null,
  minVersion: null,
  updateUrl: null,
  needBlocks: null,
};

/** The one client, wired to the folder store and the notices. Tests build their own with a socket of their own. */
export function createRealtime(socket?: RealtimeOptions['socket']) {
  return new RealtimeClient({
    url: realtimeUrl(globalThis.location),
    socket,
    handlers: {
      loadHead: async () => {
        const head = await api.getChangesHead();
        return head.ok ? parseCursor(head.value.cursor) : null;
      },
      loadListing: () => folderStore.reload(),
      refreshPendingThumbs: () => folderStore.refreshPendingThumbs(),
      onChange: applyChange,
      onThumbState: (hash, state) => folderStore.setThumbState(hash, state),
      onSignedOut: () => api.report(SESSION_ENDED),
      onProtocolError: () =>
        notices.show({
          key: 'realtime',
          variant: 'danger',
          title: 'Live updates stopped.',
          text: 'The page and the server disagree about a message. Reload the page.',
          action: 'reload',
        }),
      onUpgradeRequired: () =>
        notices.show({
          key: 'upgrade',
          variant: 'warning',
          title: 'A newer version of sunnycloud is required.',
          text: 'Reload the page to update the web app.',
          action: 'reload',
        }),
    },
  });
}

export const realtime = createRealtime();
