<script lang="ts">
  import type { ApiError, SessionList } from '$lib/api';
  import { EmptyState, Skeleton } from '$lib/ui';
  import DeviceRow from './DeviceRow.svelte';
  import SectionError from './SectionError.svelte';
  import { browserRows, type DeviceRow as Row } from './view';

  interface Props {
    sessions: SessionList | null;
    error: ApiError | null;
    onretry: () => void;
    onrevoked: (row: Row) => void;
  }

  let { sessions, error, onretry, onrevoked }: Props = $props();

  const rows = $derived(sessions ? browserRows(sessions) : []);
</script>

<section id="sessions" class="flex scroll-mt-4 flex-col gap-3" aria-labelledby="sessions-h">
  <div>
    <h2 id="sessions-h" class="text-h2">Browser sessions</h2>
    <p class="text-small text-ink-2">Browsers signed in to this web app.</p>
  </div>
  {#if error}
    <SectionError what="The browser sessions could not be loaded." {error} {onretry} />
  {:else if sessions === null}
    <div
      class="flex flex-col gap-4 rounded-card border border-line bg-raised p-4"
      aria-busy="true"
      aria-label="Loading browser sessions"
    >
      <div class="flex items-center gap-3">
        <Skeleton class="size-10 flex-none" />
        <div class="flex flex-1 flex-col gap-2">
          <Skeleton class="h-3 w-36" />
          <Skeleton class="h-2.5 w-48" />
        </div>
      </div>
    </div>
  {:else if rows.length === 0}
    <div class="rounded-card border border-line bg-raised">
      <EmptyState title="No browsers signed in">Browsers you sign in with show here.</EmptyState>
    </div>
  {:else}
    <ul
      class="divide-y divide-line rounded-card border border-line bg-raised"
      aria-label="Browser sessions"
    >
      {#each rows as row (row.key)}
        <DeviceRow {row} {onrevoked} />
      {/each}
    </ul>
  {/if}
</section>
