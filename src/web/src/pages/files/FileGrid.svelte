<script lang="ts">
  import type { NodeView } from '$lib/api';
  import NameLink from './NameLink.svelte';
  import RowActions from './RowActions.svelte';
  import Thumb from './Thumb.svelte';

  let { items, onopen }: { items: NodeView[]; onopen: (node: NodeView) => void } = $props();
</script>

<ul class="grid grid-cols-[repeat(auto-fill,minmax(9.5rem,1fr))] gap-3">
  {#each items as node (node.node_id)}
    <li class="flex min-w-0 flex-col gap-1">
      <Thumb {node} class="aspect-square w-full" />
      <div class="flex min-w-0 flex-col text-small">
        <NameLink {node} {onopen} />
        <RowActions {node} />
      </div>
    </li>
  {/each}
</ul>
