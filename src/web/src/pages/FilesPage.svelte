<script lang="ts">
  import FolderPlus from '@lucide/svelte/icons/folder-plus';
  import LayoutGrid from '@lucide/svelte/icons/layout-grid';
  import List from '@lucide/svelte/icons/list';
  import Upload from '@lucide/svelte/icons/upload';
  import { onMount, untrack } from 'svelte';
  import { folderStore, viewPreference } from '$lib/files';
  import { notices } from '$lib/notices';
  import { route } from '$lib/router';
  import { session } from '$lib/session';
  import { transfer } from '$lib/transfer';
  import { Banner, Button, EmptyState, Skeleton } from '$lib/ui';
  import AppShell from './AppShell.svelte';
  import Breadcrumb from './files/Breadcrumb.svelte';
  import FileGrid from './files/FileGrid.svelte';
  import FileList from './files/FileList.svelte';
  import Lightbox from './files/Lightbox.svelte';
  import NewFolderForm from './files/NewFolderForm.svelte';
  import TransfersPanel from './files/TransfersPanel.svelte';
  import type { NodeView } from '$lib/api';

  const folderId = $derived(route.params.id ?? null);

  let creating = $state(false);
  let opened = $state<NodeView | null>(null);
  let uploadError = $state<string | null>(null);
  let dragging = $state(false);
  let picker: HTMLInputElement | undefined = $state();

  // Opening by folder id: every visit reads the folder again (no client-side path cache).
  $effect(() => {
    creating = false;
    opened = null;
    const id = folderId;
    untrack(() => void folderStore.open(id));
  });

  onMount(() => {
    viewPreference.init();
    void session.load();
    return transfer.onUploaded((event) => {
      if (event.parentId === folderStore.folder?.node_id) folderStore.adopt(event.nodeId);
    });
  });

  async function upload(files: File[]) {
    if (files.length === 0) return;
    const parent = folderStore.folder;
    if (!parent) {
      uploadError = 'The folder is still loading. Choose the files again in a moment.';
      return;
    }
    uploadError = null;
    const result = await transfer.enqueueUpload(files, parent.node_id);
    if (!result.ok) uploadError = result.error.text;
  }

  function picked(event: Event & { currentTarget: HTMLInputElement }) {
    const input = event.currentTarget;
    void upload(input.files ? [...input.files] : []);
    input.value = '';
  }

  function dropped(event: DragEvent) {
    event.preventDefault();
    dragging = false;
    const files: File[] = [];
    let skipped = 0;
    for (const item of event.dataTransfer?.items ?? []) {
      if (item.kind !== 'file') continue;
      if (item.webkitGetAsEntry?.()?.isDirectory) {
        skipped += 1;
        continue;
      }
      const file = item.getAsFile();
      if (file) files.push(file);
    }
    if (skipped > 0) {
      notices.show({
        key: 'folders-skipped',
        variant: 'info',
        title: 'Folders are not uploaded yet.',
        text: 'Drop files, or pick them with Upload.',
        dismissible: true,
      });
    }
    void upload(files);
  }
</script>

<AppShell>
  <div
    class="flex flex-col gap-5 rounded-card {dragging
      ? 'outline-2 outline-dashed outline-action'
      : ''}"
    role="region"
    aria-label="Files"
    ondragover={(event) => {
      event.preventDefault();
      dragging = true;
    }}
    ondragleave={() => (dragging = false)}
    ondrop={dropped}
  >
    <div class="flex flex-wrap items-center gap-3">
      {#if folderStore.trail.length > 0}
        <Breadcrumb trail={folderStore.trail} />
      {:else}
        <h1 class="min-w-0 flex-1 text-h1">Files</h1>
      {/if}
      <div
        class="inline-flex rounded-control border border-line bg-raised p-0.5"
        role="group"
        aria-label="View"
      >
        <button
          type="button"
          aria-label="List view"
          aria-pressed={viewPreference.mode === 'list'}
          class="grid size-8 place-items-center rounded-control {viewPreference.mode === 'list'
            ? 'bg-action-subtle text-action-on-subtle'
            : 'text-ink-2 hover:bg-hover'}"
          onclick={() => viewPreference.set('list')}
          ><List class="size-4" aria-hidden="true" /></button
        >
        <button
          type="button"
          aria-label="Photo grid"
          aria-pressed={viewPreference.mode === 'grid'}
          class="grid size-8 place-items-center rounded-control {viewPreference.mode === 'grid'
            ? 'bg-action-subtle text-action-on-subtle'
            : 'text-ink-2 hover:bg-hover'}"
          onclick={() => viewPreference.set('grid')}
          ><LayoutGrid class="size-4" aria-hidden="true" /></button
        >
      </div>
      <Button
        variant="secondary"
        disabled={folderStore.folder === null}
        disabledReason="Open a folder first."
        onclick={() => (creating = true)}
      >
        <FolderPlus class="size-4" aria-hidden="true" />New folder
      </Button>
      <Button
        variant="primary"
        disabled={folderStore.folder === null}
        disabledReason="Open a folder first."
        onclick={() => picker?.click()}
      >
        <Upload class="size-4" aria-hidden="true" />Upload files
      </Button>
      <input
        bind:this={picker}
        type="file"
        multiple
        class="sr-only"
        tabindex="-1"
        aria-label="Choose files to upload"
        onchange={picked}
      />
    </div>

    {#if creating}
      <NewFolderForm ondone={() => (creating = false)} />
    {/if}
    {#if uploadError}
      <Banner variant="danger">
        <strong class="font-bold">Nothing was uploaded.</strong>
        {uploadError}
      </Banner>
    {/if}

    {#if folderStore.status === 'failed'}
      <Banner variant="danger">
        <strong class="font-bold">This folder could not be loaded.</strong>
        {#snippet action()}
          <Button size="sm" onclick={() => folderStore.open(folderId)}>Retry</Button>
        {/snippet}
      </Banner>
    {:else if folderStore.status !== 'ready'}
      <div class="flex flex-col gap-3" aria-busy="true" aria-label="Loading the folder">
        {#each [1, 2, 3, 4] as row (row)}
          <div class="flex items-center gap-3">
            <Skeleton class="size-10 flex-none" />
            <Skeleton class="h-3 w-2/5" />
          </div>
        {/each}
      </div>
    {:else if folderStore.items.length === 0}
      <div class="rounded-card border border-line bg-raised">
        <EmptyState title="This folder is empty">
          Drop files here or upload them. For large folders, the desktop app syncs faster and keeps
          them up to date.
          {#snippet action()}
            <Button variant="primary" onclick={() => picker?.click()}>
              <Upload class="size-4" aria-hidden="true" />Upload files
            </Button>
          {/snippet}
        </EmptyState>
      </div>
    {:else}
      {#if viewPreference.mode === 'grid'}
        <FileGrid items={folderStore.items} onopen={(node) => (opened = node)} />
      {:else}
        <FileList items={folderStore.items} onopen={(node) => (opened = node)} />
      {/if}
      {#if folderStore.next !== null}
        <div class="flex flex-col items-center gap-2">
          {#if folderStore.moreFailed}
            <p class="text-small font-semibold text-danger" role="alert">
              The next page could not be loaded.
            </p>
          {/if}
          <Button loading={folderStore.loadingMore} onclick={() => folderStore.loadMore()}
            >Load more</Button
          >
        </div>
      {/if}
    {/if}
  </div>

  {#if opened}
    <Lightbox node={opened} onclose={() => (opened = null)} />
  {/if}
  <TransfersPanel />
</AppShell>
