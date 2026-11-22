<script lang="ts">
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import Globe from '@lucide/svelte/icons/globe';
  import Laptop from '@lucide/svelte/icons/laptop';
  import Smartphone from '@lucide/svelte/icons/smartphone';
  import { api, type ApiError } from '$lib/api';
  import { Button } from '$lib/ui';
  import { errorText, type DeviceRow } from './view';

  let { row, onrevoked }: { row: DeviceRow; onrevoked: (row: DeviceRow) => void } = $props();

  const Icon = $derived(row.kind === 'desktop' ? Laptop : row.kind === 'web' ? Globe : Smartphone);
  const what = $derived(row.kind === 'web' ? 'browser' : row.kind === 'desktop' ? 'device' : 'app');
  const consequence = $derived(
    row.kind === 'web'
      ? 'It is signed out and needs to sign in again.'
      : row.kind === 'desktop'
        ? 'It stops syncing and needs a new approval to sign in again.'
        : 'It stops backing up and needs to sign in again.',
  );

  let confirming = $state(false);
  let revoking = $state(false);
  let failure = $state<ApiError | null>(null);

  async function revoke() {
    revoking = true;
    failure = null;
    const result = await (row.kind === 'desktop'
      ? api.deleteDevice(row.id)
      : api.deleteSession(row.id));
    revoking = false;
    if (result.ok) {
      onrevoked(row);
      return;
    }
    failure = result.error;
  }
</script>

<li class="flex flex-col">
  <div class="flex items-center gap-3 px-4 py-3">
    <span class="grid size-10 flex-none place-items-center rounded-control bg-sunken text-ink-2">
      <Icon class="size-5" aria-hidden="true" />
    </span>
    <div class="min-w-0 flex-1">
      <p class="flex flex-wrap items-center gap-2 text-row">
        <span class="truncate">{row.name}</span>
        {#if row.current}
          <span
            class="rounded-full bg-sun-fill px-2 py-0.5 text-small font-semibold text-sun-on"
            data-testid="current-chip">This browser</span
          >
        {/if}
      </p>
      <p class="text-small text-ink-2 tabular-nums">{row.detail}</p>
      <p class="text-small text-ink-2 md:hidden">{row.lastSeen}</p>
    </div>
    <span class="hidden w-40 text-small text-ink-2 tabular-nums md:block">{row.lastSeen}</span>
    {#if row.current}
      <span class="hidden w-[74px] sm:block" aria-hidden="true"></span>
    {:else}
      <Button
        variant="danger-ghost"
        size="sm"
        aria-label="Revoke {row.name}"
        disabled={confirming}
        onclick={() => (confirming = true)}>Revoke</Button
      >
    {/if}
  </div>
  {#if confirming}
    <div
      class="flex flex-wrap items-center gap-x-3 gap-y-2 border-t border-line bg-danger-bg px-4 py-3 text-danger-on"
      role="alert"
    >
      <CircleAlert class="size-5 flex-none" aria-hidden="true" />
      <p class="min-w-[12rem] flex-1 text-row">
        <strong class="font-bold">Revoke {row.name}?</strong>
        {consequence}
      </p>
      <div class="ml-auto flex gap-2">
        <Button
          variant="ghost"
          size="sm"
          disabled={revoking}
          onclick={() => {
            confirming = false;
            failure = null;
          }}>Cancel</Button
        >
        <Button variant="danger" size="sm" loading={revoking} onclick={revoke}>Revoke {what}</Button
        >
      </div>
      {#if failure}
        <p class="basis-full text-small font-semibold" data-testid="revoke-error">
          {errorText(failure)}
          {#if failure.requestId}(reference <code class="font-code">{failure.requestId}</code>){/if}
        </p>
      {/if}
    </div>
  {/if}
</li>
