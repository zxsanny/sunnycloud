<script lang="ts">
  import Eye from '@lucide/svelte/icons/eye';
  import EyeOff from '@lucide/svelte/icons/eye-off';
  import Link from '@lucide/svelte/icons/link';
  import type { OwnerShares } from '$lib/shares';
  import { Button, Field } from '$lib/ui';

  let { shares, oncancel }: { shares: OwnerShares; oncancel: () => void } = $props();

  let days = $state('');
  let requirePassword = $state(false);
  let password = $state('');
  let reveal = $state(false);

  const missingPassword = $derived(requirePassword && password === '');

  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (missingPassword) return;
    void shares.create({ days, password: requirePassword ? password : null });
  }
</script>

<form class="flex flex-col gap-4" aria-labelledby="new-h" onsubmit={submit}>
  <h3 id="new-h" class="text-h3">New link</h3>
  <Field
    label="Expires after (days)"
    bind:value={days}
    inputmode="numeric"
    autocomplete="off"
    placeholder="Server default"
    helper="Leave empty to use the server's default lifetime."
    error={shares.failure?.place === 'expiry' ? shares.failure.text : undefined}
  />
  <div class="flex items-center justify-between gap-4">
    <label class="text-small font-[650]" for="pw-switch">Require a password</label>
    <input
      id="pw-switch"
      type="checkbox"
      role="switch"
      class="size-5 accent-action"
      bind:checked={requirePassword}
    />
  </div>
  {#if requirePassword}
    <Field
      label="Password"
      bind:value={password}
      type={reveal ? 'text' : 'password'}
      autocomplete="new-password"
      helper="Send the password separately from the link."
    >
      <div>
        <Button
          variant="ghost"
          size="sm"
          aria-label={reveal ? 'Hide password' : 'Show password'}
          onclick={() => (reveal = !reveal)}
        >
          {#if reveal}<EyeOff class="size-4" aria-hidden="true" />Hide{:else}<Eye
              class="size-4"
              aria-hidden="true"
            />Show{/if}
        </Button>
      </div>
    </Field>
  {/if}
  {#if shares.failure?.place === 'form'}
    <p class="text-small font-semibold text-danger" role="alert">{shares.failure.text}</p>
  {/if}
  <div class="flex flex-wrap justify-end gap-3">
    <Button onclick={oncancel}>Cancel</Button>
    <Button
      type="submit"
      variant="primary"
      loading={shares.creating}
      disabled={missingPassword}
      disabledReason="Enter a password, or turn the option off."
    >
      <Link class="size-4" aria-hidden="true" />Create link
    </Button>
  </div>
</form>
