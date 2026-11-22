<script lang="ts">
  import Info from '@lucide/svelte/icons/info';
  import X from '@lucide/svelte/icons/x';
  import { onMount } from 'svelte';
  import { api, type NodeView } from '$lib/api';
  import { OwnerShares } from '$lib/shares';
  import { Banner, Button, Skeleton } from '$lib/ui';
  import CreatedPanel from './share/CreatedPanel.svelte';
  import LinkList from './share/LinkList.svelte';
  import NewLinkForm from './share/NewLinkForm.svelte';

  let { node, onclose }: { node: NodeView; onclose: () => void } = $props();

  // svelte-ignore state_referenced_locally
  const shares = new OwnerShares(api, node.node_id, globalThis.location.origin);
  let dialog: HTMLDivElement | undefined = $state();
  let closeButton: HTMLButtonElement | undefined = $state();
  let opener: Element | null = null;

  onMount(() => {
    opener = document.activeElement;
    closeButton?.focus();
    void shares.load();
    void shares.loadLimits();
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
      const focusable = [
        ...dialog.querySelectorAll<HTMLElement>('a[href], button:not(:disabled), input, select'),
      ];
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

<div class="fixed inset-0 z-40 overflow-y-auto bg-overlay p-4 lg:pt-14" role="presentation">
  <div
    bind:this={dialog}
    class="mx-auto flex w-full max-w-[560px] flex-col rounded-dialog bg-raised shadow-popup"
    role="dialog"
    aria-modal="true"
    aria-labelledby="share-h"
    tabindex="-1"
    {onkeydown}
  >
    <header class="flex h-16 items-center gap-3 border-b border-line px-5 sm:px-6">
      <h2 id="share-h" class="min-w-0 flex-1 truncate text-h2" title="Share {node.name}">
        Share {node.name}
      </h2>
      <button
        bind:this={closeButton}
        type="button"
        class="grid size-8 place-items-center rounded-control text-ink-2 hover:bg-hover"
        aria-label="Close share dialog"
        onclick={onclose}
      >
        <X class="size-4" aria-hidden="true" />
      </button>
    </header>
    <div class="flex flex-col gap-6 px-5 py-5 sm:px-6">
      {#if shares.created}
        <CreatedPanel created={shares.created} />
      {:else}
        <NewLinkForm {shares} oncancel={onclose} />
        <hr class="border-line" />
      {/if}
      <section class="flex flex-col gap-3" aria-labelledby="links-h">
        <h3 id="links-h" class="text-h3">Links for this file</h3>
        {#if shares.status === 'loading'}
          <Skeleton class="h-16 w-full" />
        {:else if shares.status === 'failed'}
          <Banner variant="danger">
            The links could not be loaded.
            {#snippet action()}
              <Button size="sm" onclick={() => void shares.load()}>Try again</Button>
            {/snippet}
          </Banner>
        {:else}
          <LinkList {shares} />
        {/if}
        <p class="flex items-start gap-2 text-small text-ink-2">
          <Info class="mt-0.5 size-4 flex-none" aria-hidden="true" />Links follow the file: they
          keep working after you rename or move it. Deleting the file ends its links.
        </p>
      </section>
    </div>
    {#if shares.created}
      <footer class="flex justify-end border-t border-line px-5 py-4 sm:px-6">
        <Button onclick={onclose}>Done</Button>
      </footer>
    {/if}
  </div>
</div>
