<script lang="ts">
  import Download from '@lucide/svelte/icons/download';
  import Link from '@lucide/svelte/icons/link';
  import ShieldCheck from '@lucide/svelte/icons/shield-check';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import type { NodeView } from '$lib/api';
  import { contentUrl, folderStore } from '$lib/files';
  import { transfer } from '$lib/transfer';
  import { Button } from '$lib/ui';
  import ShareDialog from './ShareDialog.svelte';

  let { node }: { node: NodeView } = $props();

  let sharing = $state(false);
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
        <!-- Until the row menu of the file browser (E-26/c08) calls `download` itself, this is the way to it; the
             plain link above stays as the browser's own download. -->
        <Button
          variant="ghost"
          size="sm"
          aria-label="Download {node.name} with check"
          title="Download and check"
          onclick={() => void transfer.download({ nodeId: node.node_id })}
        >
          <ShieldCheck class="size-4" aria-hidden="true" />
        </Button>
      {/if}
      <!-- Links are for files in v1.0 (06, decision 15); a folder keeps the control and says why it is off. -->
      <Button
        variant="ghost"
        size="sm"
        aria-label="Share link for {node.name}"
        title="Share link…"
        disabled={node.kind === 'folder'}
        disabledReason="Folder sharing is not available in this version"
        onclick={() => (sharing = true)}
      >
        <Link class="size-4" aria-hidden="true" />
      </Button>
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
{#if sharing}
  <ShareDialog {node} onclose={() => (sharing = false)} />
{/if}
