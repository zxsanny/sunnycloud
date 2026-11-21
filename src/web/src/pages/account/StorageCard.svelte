<script lang="ts">
  import type { ApiError, Quota } from '$lib/api';
  import { Skeleton } from '$lib/ui';
  import SectionError from './SectionError.svelte';
  import { quotaView } from './view';

  interface Props {
    quota: Quota | null;
    error: ApiError | null;
    onretry: () => void;
  }

  let { quota, error, onretry }: Props = $props();

  const view = $derived(quota ? quotaView(quota) : null);
</script>

<section
  class="flex flex-col gap-4 rounded-card bg-raised p-5 shadow-card"
  aria-labelledby="usage-h"
>
  <h2 id="usage-h" class="text-h2">Storage</h2>
  {#if error}
    <SectionError what="Storage could not be loaded." {error} {onretry} />
  {:else if view}
    <div class="flex flex-col gap-3">
      <p class="tabular-nums">
        <span class="text-h1">{view.used}</span>
        <span class="text-small text-ink-2">of {view.limit} used</span>
      </p>
      <div
        class="h-1.5 overflow-hidden rounded-full bg-sunken"
        role="progressbar"
        aria-valuenow={view.percent}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label="Storage used"
      >
        <span class="block h-full bg-action" style="width: {view.percent}%"></span>
      </div>
      <ul class="flex flex-col gap-1 text-small text-ink-2 tabular-nums">
        <li class="flex justify-between gap-3">
          <span>Reserved for uploads in progress</span>
          <span class="font-semibold text-ink">{view.reserved}</span>
        </li>
        <li class="flex justify-between gap-3">
          <span>Quota</span>
          <span class="font-semibold text-ink"
            >{view.limit}{view.serverDefault ? ' · server default' : ''}</span
          >
        </li>
      </ul>
      <p class="text-small text-ink-2">
        Only the current version of each file counts. Previews and versions kept for recovery do
        not. Need more space? Ask your administrator.
      </p>
    </div>
  {:else}
    <div class="flex flex-col gap-3" aria-busy="true" aria-label="Loading storage">
      <Skeleton class="h-7 w-40" />
      <Skeleton class="h-1.5 w-full" />
      <Skeleton class="h-3 w-3/4" />
      <Skeleton class="h-3 w-2/3" />
    </div>
  {/if}
</section>
