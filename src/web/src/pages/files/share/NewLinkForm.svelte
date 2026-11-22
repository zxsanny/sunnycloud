<script lang="ts">
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import Eye from '@lucide/svelte/icons/eye';
  import EyeOff from '@lucide/svelte/icons/eye-off';
  import Link from '@lucide/svelte/icons/link';
  import { expiryOptions, type ExpiryOption, type OwnerShares } from '$lib/shares';
  import { Button, Field } from '$lib/ui';

  let { shares, oncancel }: { shares: OwnerShares; oncancel: () => void } = $props();

  /** The key of the option the owner picked; null until they pick, so the server's default stays selected. */
  let picked = $state<string | null>(null);
  let requirePassword = $state(false);
  let password = $state('');
  let reveal = $state(false);

  const expiry = $derived(expiryOptions(shares.ttl ?? { defaultTtl: null, maxTtl: null }));
  const chosen = $derived(
    expiry.options.find((option) => keyOf(option) === picked) ?? expiry.preselected,
  );
  const expiryError = $derived(
    shares.failure?.place === 'expiry' ? shares.failure.text : undefined,
  );
  const missingPassword = $derived(requirePassword && password === '');

  const keyOf = (option: ExpiryOption) => String(option.seconds ?? 'default');

  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (missingPassword) return;
    void shares.create({
      expiresIn: chosen.seconds,
      password: requirePassword ? password : null,
    });
  }
</script>

<form class="flex flex-col gap-4" aria-labelledby="new-h" onsubmit={submit}>
  <h3 id="new-h" class="text-h3">New link</h3>
  <div class="flex flex-col gap-1.5">
    <label class="text-small font-[650] text-ink" for="share-expires">Expires</label>
    <select
      id="share-expires"
      class="h-10 w-full rounded-control border bg-raised px-3 text-ink {expiryError
        ? 'border-danger'
        : 'border-line-strong'}"
      aria-invalid={expiryError ? 'true' : undefined}
      aria-describedby="share-expires-error share-expires-help"
      value={keyOf(chosen)}
      onchange={(event) => (picked = event.currentTarget.value)}
    >
      {#each expiry.options as option (keyOf(option))}
        <option value={keyOf(option)}>{option.label}</option>
      {/each}
    </select>
    {#if expiryError}
      <p
        id="share-expires-error"
        class="flex items-start gap-1.5 text-[0.8125rem] font-semibold text-danger"
      >
        <CircleAlert class="mt-px size-4 flex-none" aria-hidden="true" />{expiryError}
      </p>
    {/if}
    {#if expiry.helper}
      <p id="share-expires-help" class="text-[0.8125rem] text-ink-2">{expiry.helper}</p>
    {/if}
  </div>
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
