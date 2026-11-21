<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type AdminHealth, type ApiError } from '$lib/api';
  import { Banner, Button, Skeleton } from '$lib/ui';
  import SectionError from '../account/SectionError.svelte';
  import { readiness } from './view';

  let health = $state<AdminHealth | null>(null);
  let error = $state<ApiError | null>(null);
  let checking = $state(false);

  const ready = $derived(health ? readiness(health) : null);

  async function load() {
    checking = true;
    error = null;
    const result = await api.getAdminHealth();
    checking = false;
    if (result.ok) health = result.value;
    else error = result.error;
  }

  onMount(() => void load());
</script>

<div id="health-panel" role="tabpanel" aria-labelledby="health-tab" class="flex flex-col gap-5">
  {#if error}
    <SectionError what="The health checks could not be read." {error} onretry={load} />
  {/if}
  {#if health === null && !error}
    <div
      class="flex flex-col gap-3 rounded-card border border-line bg-raised p-4"
      aria-busy="true"
      aria-label="Loading health"
    >
      <Skeleton class="h-10 w-full" />
      <Skeleton class="h-10 w-full" />
    </div>
  {:else if health && ready}
    <Banner
      variant={ready.state === 'ok' ? 'success' : ready.state === 'degraded' ? 'warning' : 'danger'}
    >
      {#if ready.state === 'ok'}
        <strong class="font-bold">Ready.</strong> All checks pass.
      {:else if ready.state === 'degraded'}
        <strong class="font-bold">Degraded.</strong> Needs attention: {ready.failing.join(', ')}.
      {:else}
        <strong class="font-bold">Down.</strong> Failing: {ready.failing.join(', ')}.
      {/if}
      {#snippet action()}
        <Button size="sm" loading={checking} onclick={load}>Check again</Button>
      {/snippet}
    </Banner>
    <div class="overflow-x-auto rounded-card border border-line bg-raised">
      <table class="w-full text-left text-row" aria-label="Health checks">
        <thead class="text-small text-ink-2">
          <tr>
            <th scope="col" class="px-4 py-2 font-[650]">Check</th>
            <th scope="col" class="px-4 py-2 font-[650]">Status</th>
            <th scope="col" class="px-4 py-2 font-[650]">Detail</th>
          </tr>
        </thead>
        <tbody>
          {#each health.items as item (item.name)}
            <tr class="border-t border-line align-top">
              <td class="px-4 py-3 font-code text-small">{item.name}</td>
              <td class="px-4 py-3 text-small font-semibold">
                {item.status === 'ok' ? 'OK' : item.status === 'degraded' ? 'Degraded' : 'Down'}
              </td>
              <td class="px-4 py-3 text-small text-ink-2">{item.detail}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>
