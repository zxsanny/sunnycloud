<script lang="ts">
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import Clock from '@lucide/svelte/icons/clock';
  import X from '@lucide/svelte/icons/x';
  import { Button } from '$lib/ui';

  interface Props {
    kind: 'approved' | 'denied' | 'expired';
    /** The device's name, as the server sent it; shown by the approved and denied pages. */
    device?: string;
    /** Takes the visitor back to code entry from the expired page. */
    onretry?: () => void;
  }

  let { kind, device = '', onretry }: Props = $props();
</script>

{#if kind === 'approved'}
  <section
    class="flex flex-col items-center gap-4 rounded-card border border-line bg-raised px-6 py-10 text-center sm:px-10"
    aria-labelledby="outcome-h"
    role="status"
  >
    <span class="grid size-14 place-items-center rounded-full bg-success-bg text-success">
      <CircleCheck class="size-7" aria-hidden="true" />
    </span>
    <h1 id="outcome-h" class="text-h1">{device} is connected</h1>
    <p class="text-ink-2">
      You can close this page. The desktop app finishes signing in by itself.
    </p>
    <a class="font-semibold text-link underline" href="/account">Manage devices</a>
  </section>
{:else if kind === 'denied'}
  <section
    class="flex flex-col items-center gap-4 rounded-card border border-line bg-raised px-6 py-10 text-center sm:px-10"
    aria-labelledby="outcome-h"
    role="status"
  >
    <span class="grid size-14 place-items-center rounded-full bg-sunken text-ink-2">
      <X class="size-7" aria-hidden="true" />
    </span>
    <h1 id="outcome-h" class="text-h1">Request denied</h1>
    <p class="text-ink-2">
      {device} wasn't connected, and this code can't be used again.
    </p>
    <a class="font-semibold text-link underline" href="/account">Manage devices</a>
  </section>
{:else}
  <section
    class="flex flex-col items-center gap-4 rounded-card border border-line bg-raised px-6 py-10 text-center sm:px-10"
    aria-labelledby="outcome-h"
  >
    <span class="grid size-14 place-items-center rounded-full bg-sunken text-ink-2">
      <Clock class="size-7" aria-hidden="true" />
    </span>
    <h1 id="outcome-h" class="text-h1">This code has expired or isn't valid</h1>
    <p class="text-ink-2">
      Codes only work for a few minutes, and a code that was already used or denied can't be used
      again. To get a new one, open Account in the sunnycloud desktop app and sign in again.
    </p>
    <Button variant="primary" size="lg" onclick={onretry}>Enter a new code</Button>
  </section>
{/if}
