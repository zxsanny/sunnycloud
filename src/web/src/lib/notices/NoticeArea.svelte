<script lang="ts">
  import NoticeItem from './NoticeItem.svelte';
  import { notices } from './notices.svelte';
</script>

{#if notices.items.length > 0}
  <div class="pointer-events-none fixed inset-x-0 top-0 z-50 flex flex-col items-center gap-3 p-4">
    {#each notices.items as notice (notice.key)}
      <div class="pointer-events-auto w-full max-w-[720px] shadow-popup">
        <NoticeItem {notice} />
      </div>
    {/each}
  </div>
{/if}
