<script lang="ts">
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import type { NodeView } from '$lib/api';

  let { trail }: { trail: NodeView[] } = $props();
</script>

<nav aria-label="Breadcrumb" class="min-w-0 flex-1">
  <ol class="flex min-w-0 items-center gap-1 text-small text-ink-2">
    {#each trail as node, at (node.node_id)}
      {@const last = at === trail.length - 1}
      <li class="flex min-w-0 items-center gap-1">
        {#if at > 0}<ChevronRight class="size-4 flex-none" aria-hidden="true" />{/if}
        {#if last}
          <span class="truncate font-semibold text-ink" aria-current="page" title={node.name}>
            {node.parent_id === null ? 'Files' : node.name}
          </span>
        {:else}
          <a
            class="truncate rounded-control text-link hover:underline"
            href={node.parent_id === null ? '/' : `/folders/${node.node_id}`}
          >
            {node.parent_id === null ? 'Files' : node.name}
          </a>
        {/if}
      </li>
    {/each}
  </ol>
</nav>
