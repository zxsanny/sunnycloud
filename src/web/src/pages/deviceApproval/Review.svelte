<script lang="ts">
  import Check from '@lucide/svelte/icons/check';
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import Clock from '@lucide/svelte/icons/clock';
  import Laptop from '@lucide/svelte/icons/laptop';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import X from '@lucide/svelte/icons/x';
  import { api, type ApiError, type DeviceGrantApproval, type DeviceGrantDecision } from '$lib/api';
  import { Banner, Button } from '$lib/ui';
  import { errorText } from '../account/view';
  import LocationTable from './LocationTable.svelte';
  import { actionState, countdownText, platformText, remainingSeconds } from './view';

  interface Props {
    approval: DeviceGrantApproval;
    code: string;
    ondecided: (decision: DeviceGrantDecision) => void;
    /** The code ran out, here or on the server. */
    onexpired: () => void;
  }

  let { approval, code, ondecided, onexpired }: Props = $props();

  let confirmed = $state(false);
  let deciding = $state<DeviceGrantDecision | null>(null);
  let failure = $state<ApiError | null>(null);
  let now = $state(Date.now());

  const left = $derived(remainingSeconds(approval.expires_at, now));
  const actions = $derived(actionState(approval.mismatch, confirmed));

  // A local display of the server's expiry; it asks the server nothing.
  $effect(() => {
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 250);
    return () => clearInterval(timer);
  });

  $effect(() => {
    if (left === 0) onexpired();
  });

  async function decide(decision: DeviceGrantDecision) {
    // Set before the first await: a second click finds the decision already under way.
    if (deciding !== null || left === 0) return;
    deciding = decision;
    failure = null;
    const result = await api.patchDeviceGrant(approval.grant_id, decision);
    if (result.ok) {
      ondecided(decision);
      return;
    }
    if (result.error.status === 404) {
      onexpired();
      return;
    }
    deciding = null;
    failure = result.error;
  }
</script>

<section
  class="flex flex-col gap-6 rounded-card border border-line bg-raised p-6 sm:p-8"
  aria-labelledby="review-h"
>
  <div class="flex flex-col gap-2">
    <h1 id="review-h" class="text-h1">Approve this sign-in?</h1>
    <p class="flex items-center gap-2 text-small text-ink-2 tabular-nums" data-testid="countdown">
      <Clock class="size-4" aria-hidden="true" />Code expires in
      <span class="font-bold text-ink">{countdownText(left)}</span>
    </p>
  </div>

  {#if approval.mismatch}
    <Banner variant="warning" icon={TriangleAlert}>
      <strong class="font-bold">This request comes from a different place than you.</strong>
      If you didn't just start signing in on this device yourself, deny the request.
    </Banner>
  {/if}

  <div class="flex items-center gap-4 rounded-card bg-sunken p-4">
    <span class="grid size-12 flex-none place-items-center rounded-control bg-raised text-ink-2">
      <Laptop class="size-6" aria-hidden="true" />
    </span>
    <div class="min-w-0 flex-1">
      <p class="truncate text-h3" title={approval.device} data-testid="device-name">
        {approval.device}
      </p>
      <p class="text-small text-ink-2" data-testid="device-platform">
        Desktop app · {platformText(approval.platform)}
      </p>
    </div>
    <span
      class="hidden flex-none rounded-full border border-line-strong px-2.5 py-0.5 font-code text-small uppercase sm:inline-flex"
      title="Code you entered">{code}</span
    >
  </div>

  <section class="flex flex-col gap-2" aria-labelledby="loc-h">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <h2 id="loc-h" class="text-label font-bold tracking-wide text-ink-2 uppercase">Location</h2>
      {#if approval.mismatch}
        <span
          class="inline-flex items-center gap-1.5 rounded-full bg-warning-bg px-2.5 py-0.5 text-small font-semibold text-warning-on"
        >
          <TriangleAlert class="size-4" aria-hidden="true" />Different location
        </span>
      {:else}
        <span
          class="inline-flex items-center gap-1.5 rounded-full bg-success-bg px-2.5 py-0.5 text-small font-semibold text-success"
        >
          <CircleCheck class="size-4" aria-hidden="true" />Same location
        </span>
      {/if}
    </div>
    <LocationTable {approval} />
  </section>

  <section class="flex flex-col gap-3 rounded-card bg-sunken p-4" aria-labelledby="can-h">
    <h2 id="can-h" class="text-h3">What this device can do</h2>
    <ul class="flex flex-col gap-2 text-row">
      <li class="flex items-start gap-2.5">
        <Check class="mt-0.5 size-4 flex-none text-success" aria-hidden="true" />
        <span>Sync, upload and download your files</span>
      </li>
      <li class="flex items-start gap-2.5">
        <X class="mt-0.5 size-4 flex-none text-ink-2" aria-hidden="true" />
        <span>It can't create share links, connect other devices or change your account</span>
      </li>
    </ul>
    <p class="text-small text-ink-2">You can disconnect it at any time in Devices.</p>
  </section>

  {#if approval.mismatch}
    <div class="flex flex-col gap-1">
      <label class="flex items-center gap-3 text-row">
        <input
          type="checkbox"
          class="size-4 accent-[var(--action)]"
          bind:checked={confirmed}
          disabled={deciding !== null}
          aria-describedby="approve-reason"
        />
        <span>I started this sign-in on that device</span>
      </label>
      <p id="approve-reason" class="pl-7 text-small text-ink-2">
        Approve stays off until you tick this box.
      </p>
    </div>
  {/if}

  <div class="flex flex-col-reverse gap-3 sm:flex-row sm:justify-end">
    {#if actions.primary === 'approve'}
      <Button
        size="lg"
        loading={deciding === 'deny'}
        disabled={deciding !== null}
        onclick={() => decide('deny')}>Deny</Button
      >
      <Button
        variant="primary"
        size="lg"
        loading={deciding === 'approve'}
        disabled={deciding !== null}
        onclick={() => decide('approve')}><Check class="size-5" aria-hidden="true" />Approve</Button
      >
    {:else}
      <Button
        size="lg"
        loading={deciding === 'approve'}
        disabled={!actions.approveEnabled || deciding !== null}
        disabledReason="Tick “I started this sign-in on that device” to approve"
        aria-describedby="approve-reason"
        onclick={() => decide('approve')}>Approve</Button
      >
      <Button
        variant="primary"
        size="lg"
        loading={deciding === 'deny'}
        disabled={deciding !== null}
        onclick={() => decide('deny')}><X class="size-5" aria-hidden="true" />Deny request</Button
      >
    {/if}
  </div>
  {#if failure}
    <p class="text-small font-semibold text-danger" role="alert" data-testid="decision-error">
      {errorText(failure)}
      {#if failure.requestId}(reference <code class="font-code">{failure.requestId}</code>){/if}
    </p>
  {/if}
</section>
