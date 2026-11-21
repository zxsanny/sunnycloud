<script lang="ts">
  import Laptop from '@lucide/svelte/icons/laptop';
  import Shield from '@lucide/svelte/icons/shield';
  import { api, type ApiError, type DeviceGrantApproval } from '$lib/api';
  import { Button, Field } from '$lib/ui';
  import { countdownText, secondsUntil } from './view';

  interface Props {
    /** The grant the typed code names, with the code as typed. */
    onfound: (approval: DeviceGrantApproval, code: string) => void;
    /** The server knows no such code: wrong, expired or already decided look the same. */
    oninvalid: () => void;
  }

  let { onfound, oninvalid }: Props = $props();

  let code = $state('');
  let looking = $state(false);
  let failure = $state<string | null>(null);
  /** The time the wrong-code limit ends, from the server's Retry-After. */
  let blockedUntil = $state<number | null>(null);
  let now = $state(Date.now());

  const waiting = $derived(blockedUntil === null ? 0 : secondsUntil(blockedUntil, now));

  // A local display of the server's Retry-After; it asks the server nothing.
  $effect(() => {
    if (blockedUntil === null) return;
    now = Date.now();
    const timer = setInterval(() => {
      now = Date.now();
      if (blockedUntil !== null && now >= blockedUntil) blockedUntil = null;
    }, 250);
    return () => clearInterval(timer);
  });

  function describe(error: ApiError): string {
    if (error.kind === 'network') return 'The server could not be reached. Try again.';
    if (error.status === 429) return 'Too many wrong codes. Try again later.';
    if (error.status === 403) return 'This page needs a browser sign-in, not a device sign-in.';
    return 'The server could not look up this code. Try again.';
  }

  async function lookup(event: SubmitEvent) {
    event.preventDefault();
    if (looking || waiting > 0 || code.trim() === '') return;
    looking = true;
    failure = null;
    const typed = code.trim();
    const result = await api.getDeviceGrant(typed);
    looking = false;
    if (result.ok) {
      onfound(result.value, typed);
      return;
    }
    const { error } = result;
    if (error.status === 404 || error.status === 400) {
      oninvalid();
    } else if (error.status === 429 && error.retryAfterSeconds !== null) {
      blockedUntil = Date.now() + error.retryAfterSeconds * 1000;
    } else {
      failure = describe(error);
    }
  }
</script>

<section
  class="flex flex-col gap-6 rounded-card border border-line bg-raised p-6 sm:p-8"
  aria-labelledby="enter-h"
>
  <span class="grid size-14 flex-none place-items-center rounded-full bg-action-subtle text-action">
    <Laptop class="size-7" aria-hidden="true" />
  </span>
  <div class="flex flex-col gap-2">
    <h1 id="enter-h" class="text-h1">Connect a device</h1>
    <p class="text-ink-2">Enter the code that the sunnycloud desktop app shows when you sign in.</p>
  </div>
  <form class="flex flex-col gap-4" onsubmit={lookup}>
    <Field
      label="Code"
      bind:value={code}
      mono
      placeholder="XXXX-XXXX"
      autocomplete="off"
      autocapitalize="characters"
      spellcheck={false}
      helper="You'll find it in the desktop app under Account."
      error={failure ?? undefined}
      class="h-[52px] text-[1.25rem] font-semibold uppercase tracking-[0.14em]"
    />
    {#if waiting > 0}
      <p class="text-small font-semibold text-warning-on" role="alert" data-testid="code-limit">
        Too many wrong codes. Try again in <span class="tabular-nums">{countdownText(waiting)}</span
        >.
      </p>
    {/if}
    <Button
      type="submit"
      variant="primary"
      size="lg"
      block
      loading={looking}
      disabled={code.trim() === '' || waiting > 0}
      disabledReason={waiting > 0 ? 'Wait until the limit ends.' : 'Enter the code first.'}
    >
      Continue
    </Button>
  </form>
  <p class="flex items-start gap-2 text-small text-ink-2">
    <Shield class="mt-0.5 size-4 flex-none" aria-hidden="true" />Only enter a code from a device you
    are signing in on right now.
  </p>
</section>
