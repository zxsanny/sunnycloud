<script lang="ts">
  import type { NodeView } from '$lib/api';
  import { contentUrl, folderStore, lightboxSource } from '$lib/files';

  let { node, onopen }: { node: NodeView; onopen: (node: NodeView) => void } = $props();

  const thumb = $derived(
    node.content_hash ? (folderStore.thumbs[node.content_hash] ?? 'pending') : 'none',
  );
  // The server's media type and the thumbnail state decide; the file name never does.
  const viewable = $derived(lightboxSource(node, thumb).kind !== 'none');
</script>

{#if node.kind === 'folder'}
  <a
    class="truncate font-medium text-ink hover:underline"
    href="/folders/{node.node_id}"
    title={node.name}
  >
    {node.name}
  </a>
{:else if viewable}
  <button
    type="button"
    class="truncate text-left font-medium text-ink hover:underline"
    title={node.name}
    onclick={() => onopen(node)}
  >
    {node.name}
  </button>
{:else}
  <a
    class="truncate font-medium text-ink hover:underline"
    href={contentUrl(node.node_id)}
    download={node.name}
    title={node.name}
  >
    {node.name}
  </a>
{/if}
