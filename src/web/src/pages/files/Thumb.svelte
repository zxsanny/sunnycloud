<script lang="ts">
  import File from '@lucide/svelte/icons/file';
  import Film from '@lucide/svelte/icons/film';
  import Folder from '@lucide/svelte/icons/folder';
  import ImageIcon from '@lucide/svelte/icons/image';
  import type { NodeView } from '$lib/api';
  import { folderStore, previewKind, thumbnailUrl } from '$lib/files';
  import { Skeleton } from '$lib/ui';

  let { node, class: extra = '' }: { node: NodeView; class?: string } = $props();

  const kind = $derived(previewKind(node.media_type));
  const hash = $derived(node.content_hash);
  // Unknown state (the first-load answer has not arrived yet) looks like pending.
  const shown = $derived(
    node.kind === 'folder' || kind === 'none' || !hash
      ? 'none'
      : (folderStore.thumbs[hash] ?? 'pending'),
  );
  let broken = $state(false);
  const Placeholder = $derived(kind === 'video' ? Film : kind === 'image' ? ImageIcon : File);
</script>

<span
  class="relative grid flex-none place-items-center overflow-hidden rounded-card bg-sunken text-ink-3 {extra}"
>
  {#if node.kind === 'folder'}
    <Folder class="size-1/2 text-action" aria-label="Folder" />
  {:else if shown === 'ready' && !broken}
    <img
      class="size-full object-cover"
      src={thumbnailUrl(node.node_id, 'small')}
      alt=""
      loading="lazy"
      decoding="async"
      onerror={() => (broken = true)}
    />
    {#if kind === 'video'}
      <span
        class="absolute bottom-1 left-1 inline-flex h-5 items-center gap-1 rounded-full bg-raised px-2 text-label text-ink uppercase"
        ><Film class="size-3" aria-hidden="true" />Video</span
      >
    {/if}
  {:else if shown === 'pending' && !broken}
    <Skeleton class="absolute inset-0 rounded-none" />
    <span class="sr-only">Preview pending</span>
  {:else}
    <span title="No preview" class="grid size-full place-items-center">
      <Placeholder class="size-1/2" aria-hidden="true" />
      <span class="sr-only">No preview</span>
    </span>
  {/if}
</span>
