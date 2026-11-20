<script lang="ts">
  import Download from '@lucide/svelte/icons/download';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import type { NodeView } from '$lib/api';
  import { contentUrl, folderStore } from '$lib/files';
  import { Button } from '$lib/ui';

  let { node }: { node: NodeView } = $props();

  let confirming = $state(false);
  let deleting = $state(false);
  let refusal = $state<string | null>(null);

  const REFUSALS: Record<string, string> = {
    edited_since: 'This file changed elsewhere. Reload the folder and try again.',
    not_found: 'This item no longer exists.',
    forbidden: 'You cannot delete this item.',
  };

  async function remove() {
    deleting = true;
    refusal = null;
    const result = await folderStore.deleteNode(node);
    deleting = false;
    if (result.ok) return;
    confirming = false;
    // Faults the app already reports (server, network, session) show as notices.
    refusal = REFUSALS[result.error.code] ?? 'The item could not be deleted.';
  }
</script>

<div class="flex flex-col items-end gap-1">
  {#if confirming}
    <div class="flex items-center gap-2" role="group" aria-label="Confirm delete">
      <span class="text-small text-ink-2">Delete this {node.kind}?</span>
      <Button variant="danger" size="sm" loading={deleting} onclick={remove}>Delete</Button>
      <Button variant="ghost" size="sm" disabled={deleting} onclick={() => (confirming = false)}>
        Cancel
      </Button>
    </div>
  {:else}
    <div class="flex items-center gap-1">
      {#if node.kind === 'file'}
        <a
          class="grid size-8 place-items-center rounded-control text-ink-2 hover:bg-hover"
          href={contentUrl(node.node_id)}
          download={node.name}
          aria-label="Download {node.name}"
          title="Download"
        >
          <Download class="size-4" aria-hidden="true" />
        </a>
      {/if}
      <Button
        variant="danger-ghost"
        size="sm"
        aria-label="Delete {node.name}"
        title="Delete"
        onclick={() => (confirming = true)}
      >
        <Trash2 class="size-4" aria-hidden="true" />
      </Button>
    </div>
  {/if}
  {#if refusal}
    <p class="text-small font-semibold text-danger" role="alert">{refusal}</p>
  {/if}
</div>
