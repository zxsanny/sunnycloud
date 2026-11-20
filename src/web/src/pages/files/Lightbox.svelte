<script lang="ts">
  import Download from '@lucide/svelte/icons/download';
  import X from '@lucide/svelte/icons/x';
  import { onMount } from 'svelte';
  import type { NodeView } from '$lib/api';
  import { contentUrl, folderStore, lightboxSource } from '$lib/files';

  let { node, onclose }: { node: NodeView; onclose: () => void } = $props();

  const source = $derived(
    lightboxSource(
      node,
      node.content_hash ? (folderStore.thumbs[node.content_hash] ?? 'pending') : 'none',
    ),
  );
  let dialog: HTMLDivElement | undefined = $state();
  let closeButton: HTMLButtonElement | undefined = $state();
  let opener: Element | null = null;

  onMount(() => {
    opener = document.activeElement;
    closeButton?.focus();
    return () => {
      if (opener instanceof HTMLElement) opener.focus();
    };
  });

  /** Escape closes; Tab stays inside the dialog. */
  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      onclose();
    } else if (event.key === 'Tab' && dialog) {
      const focusable = [...dialog.querySelectorAll<HTMLElement>('a[href], button')];
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first?.focus();
      }
    }
  }
</script>

<div class="fixed inset-0 z-40 grid place-items-center bg-overlay p-4" role="presentation">
  <div
    bind:this={dialog}
    class="flex max-h-full w-full max-w-[960px] flex-col gap-3 rounded-dialog bg-raised p-4 shadow-popup"
    role="dialog"
    aria-modal="true"
    aria-label={node.name}
    tabindex="-1"
    {onkeydown}
  >
    <header class="flex items-center gap-2">
      <h2 class="min-w-0 flex-1 truncate text-h3" title={node.name}>{node.name}</h2>
      <a
        class="inline-flex h-8 items-center gap-1.5 rounded-control px-3 text-small font-[650] text-ink hover:bg-hover"
        href={contentUrl(node.node_id)}
        download={node.name}
      >
        <Download class="size-4" aria-hidden="true" />Download
      </a>
      <button
        bind:this={closeButton}
        type="button"
        class="grid size-8 place-items-center rounded-control text-ink-2 hover:bg-hover"
        aria-label="Close"
        onclick={onclose}
      >
        <X class="size-4" aria-hidden="true" />
      </button>
    </header>
    {#if source.kind === 'none'}
      <p class="py-10 text-center text-small text-ink-2">
        No preview is available for this file. Download it to open it.
      </p>
    {:else}
      <img
        class="max-h-[70vh] w-full rounded-card object-contain"
        src={source.url}
        alt={node.name}
      />
      {#if source.kind === 'poster'}
        <p class="text-small text-ink-2">
          Video playback is not available yet. This is the poster frame.
        </p>
      {/if}
    {/if}
  </div>
</div>
