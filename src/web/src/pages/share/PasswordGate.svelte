<script lang="ts">
  import Lock from '@lucide/svelte/icons/lock';
  import type { PublicShareState } from '$lib/shares';
  import { Button, Field } from '$lib/ui';
  import RateLimitMessage from './RateLimitMessage.svelte';

  let { share }: { share: PublicShareState } = $props();

  // The password lives in this field until the request is sent; nothing else holds it.
  let password = $state('');

  const blocked = $derived(share.blockedUntil !== null);
  const failure = $derived(
    share.failure === 'wrong'
      ? "That password isn't right. Check it and try again."
      : share.failure === 'failed'
        ? 'The password could not be checked. Try again.'
        : share.failure === 'unseen'
          ? 'The password was accepted, but the server did not show the file. Try again later.'
          : undefined,
  );

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (password === '') return;
    await share.unlock(password);
  }
</script>

<section
  class="mx-auto flex w-full max-w-[460px] flex-col gap-5 rounded-card border border-line bg-raised p-6 shadow-card sm:p-8"
  aria-labelledby="pw-h"
>
  <span class="grid size-14 place-items-center rounded-full bg-action-subtle text-action">
    <Lock class="size-7" aria-hidden="true" />
  </span>
  <div class="flex flex-col gap-2">
    <h1 id="pw-h" class="text-h1">This link is protected</h1>
    <p class="text-ink-2">Enter the password you were given with this link.</p>
  </div>
  {#if share.blockedUntil !== null}
    <RateLimitMessage until={share.blockedUntil} onend={() => share.endBlock()} />
  {/if}
  <form class="flex flex-col gap-4" onsubmit={submit}>
    <Field
      label="Password"
      type="password"
      bind:value={password}
      autocomplete="current-password"
      error={failure}
    />
    <Button
      type="submit"
      variant="primary"
      size="lg"
      block
      loading={share.checking}
      disabled={blocked || password === ''}
      disabledReason={blocked ? 'Wait until the countdown ends' : 'Enter the password first'}
    >
      <Lock class="size-4" aria-hidden="true" />Unlock
    </Button>
    {#if blocked}
      <p class="text-center text-small">Unlock is off until the countdown ends.</p>
    {/if}
  </form>
</section>
