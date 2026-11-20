<script lang="ts">
  import type { NodeView } from '$lib/api';
  import { formatBytes, formatDate } from '$lib/format';
  import NameLink from './NameLink.svelte';
  import RowActions from './RowActions.svelte';
  import Thumb from './Thumb.svelte';

  let { items, onopen }: { items: NodeView[]; onopen: (node: NodeView) => void } = $props();
</script>

<table class="w-full border-collapse text-left">
  <thead class="text-label text-ink-2 uppercase">
    <tr class="border-b border-line">
      <th class="px-2 py-2 font-bold" scope="col">Name</th>
      <th class="hidden px-2 py-2 text-right font-bold sm:table-cell" scope="col">Size</th>
      <th class="hidden px-2 py-2 font-bold md:table-cell" scope="col">Modified</th>
      <th class="px-2 py-2" scope="col"><span class="sr-only">Actions</span></th>
    </tr>
  </thead>
  <tbody>
    {#each items as node (node.node_id)}
      <tr class="h-[52px] border-b border-line hover:bg-hover">
        <td class="max-w-0 px-2">
          <div class="flex min-w-0 items-center gap-3">
            <Thumb {node} class="size-10" />
            <div class="flex min-w-0 flex-col text-row">
              <NameLink {node} {onopen} />
              <span class="text-small text-ink-2 sm:hidden">
                {node.kind === 'file' && node.size !== null && node.size !== undefined
                  ? formatBytes(node.size)
                  : ''}
              </span>
            </div>
          </div>
        </td>
        <td class="hidden px-2 text-right text-small text-ink-2 sm:table-cell">
          {node.kind === 'file' && node.size !== null && node.size !== undefined
            ? formatBytes(node.size)
            : ''}
        </td>
        <td class="hidden px-2 text-small text-ink-2 md:table-cell"
          >{formatDate(node.updated_at)}</td
        >
        <td class="px-2 text-right"><RowActions {node} /></td>
      </tr>
    {/each}
  </tbody>
</table>
