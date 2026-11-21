<script lang="ts">
  import type { ApiError, DeviceList } from '$lib/api';
  import { EmptyState, Skeleton } from '$lib/ui';
  import DeviceRow from './DeviceRow.svelte';
  import SectionError from './SectionError.svelte';
  import { deviceRows } from './view';

  interface Props {
    devices: DeviceList | null;
    error: ApiError | null;
    onretry: () => void;
    onrevoked: (id: string) => void;
  }

  let { devices, error, onretry, onrevoked }: Props = $props();

  const rows = $derived(devices ? deviceRows(devices) : []);
</script>

<section id="devices" class="flex scroll-mt-4 flex-col gap-3" aria-labelledby="devices-h">
  <div>
    <h2 id="devices-h" class="text-h2">Devices</h2>
    <p class="text-small text-ink-2">
      Desktop apps signed in to your account. Revoking signs the app out within a few seconds.
    </p>
  </div>
  {#if error}
    <SectionError what="The devices could not be loaded." {error} {onretry} />
  {:else if devices === null}
    <div
      class="flex flex-col gap-4 rounded-card border border-line bg-raised p-4"
      aria-busy="true"
      aria-label="Loading devices"
    >
      {#each [1, 2] as row (row)}
        <div class="flex items-center gap-3">
          <Skeleton class="size-10 flex-none" />
          <div class="flex flex-1 flex-col gap-2">
            <Skeleton class="h-3 w-32" />
            <Skeleton class="h-2.5 w-52" />
          </div>
        </div>
      {/each}
    </div>
  {:else if rows.length === 0}
    <div class="rounded-card border border-line bg-raised">
      <EmptyState title="No apps signed in yet">
        Install the desktop app to keep folders in sync, or the Android app to back up your photos.
      </EmptyState>
    </div>
  {:else}
    <ul class="divide-y divide-line rounded-card border border-line bg-raised" aria-label="Devices">
      {#each rows as row (row.id)}
        <DeviceRow {row} {onrevoked} />
      {/each}
    </ul>
  {/if}
</section>
