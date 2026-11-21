<script lang="ts">
  import Clock from '@lucide/svelte/icons/clock';
  import type { PublicShareState } from '$lib/shares';
  import { Button } from '$lib/ui';
  import RateLimitMessage from './RateLimitMessage.svelte';

  let { share }: { share: PublicShareState } = $props();
</script>

<!-- The server's limit on unknown links answered the first look: nothing is known about the link yet. -->
<section
  class="mx-auto flex w-full max-w-[520px] flex-col items-center gap-4 rounded-card border border-line bg-raised px-6 py-10 text-center shadow-card sm:px-10"
  aria-labelledby="limit-h"
>
  <span class="grid size-14 place-items-center rounded-full bg-sunken text-ink-2">
    <Clock class="size-7" aria-hidden="true" />
  </span>
  <h1 id="limit-h" class="text-h1">Please wait a moment</h1>
  {#if share.blockedUntil !== null}
    <RateLimitMessage until={share.blockedUntil} onend={() => share.endBlock()} />
  {/if}
  <Button
    variant="primary"
    disabled={share.blockedUntil !== null}
    disabledReason="Wait until the countdown ends"
    onclick={() => void share.open()}
  >
    Try again
  </Button>
</section>
