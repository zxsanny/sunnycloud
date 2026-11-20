<script lang="ts">
  import Check from '@lucide/svelte/icons/check';
  import Info from '@lucide/svelte/icons/info';
  import { onMount } from 'svelte';
  import { api, isAppLevelError, type ProviderList } from '$lib/api';
  import { beginLogin, describeLoginError } from '$lib/auth';
  import { followExternal, navigate, route } from '$lib/router';
  import { Banner, Button, CopyButton, Field, Skeleton } from '$lib/ui';
  import AuthLayout from './AuthLayout.svelte';
  import ProviderButtons from './ProviderButtons.svelte';

  const SETUP_COMMAND = 'docker compose exec api sunnycloud admin setup-token';

  const reason = route.query.get('error');
  // `begin` has no token error: a bad token surfaces after the provider round trip as invitation_required.
  const tokenRejected = reason === 'invitation_required';
  const failure = tokenRejected ? null : describeLoginError(reason);

  let providers = $state<ProviderList | null>(null);
  let loadFailed = $state(false);
  // The token lives only here, in component memory, until the login attempt body carries it.
  let token = $state('');
  let step = $state<1 | 2>(1);
  let showTokenError = $state(tokenRejected);
  let starting = $state<string | null>(null);
  let startFailed = $state(false);

  const trimmed = $derived(token.trim());

  async function load() {
    loadFailed = false;
    const list = await api.getAuthProviders();
    if (!list.ok) {
      loadFailed = true;
      return;
    }
    if (!list.value.setup_required) {
      navigate(`/login${route.search}`, { replace: true });
      return;
    }
    providers = list.value;
  }

  function next(event: SubmitEvent) {
    event.preventDefault();
    if (trimmed === '') return;
    showTokenError = false;
    step = 2;
  }

  async function start(provider: string) {
    starting = provider;
    startFailed = false;
    const outcome = await beginLogin({
      provider,
      returnTo: route.query.get('return'),
      setupToken: trimmed,
    });
    if (outcome.ok) {
      followExternal(outcome.authorizationUrl);
      return;
    }
    starting = null;
    startFailed = outcome.error === null || !isAppLevelError(outcome.error);
  }

  onMount(load);
</script>

<svelte:window
  onpageshow={(event) => {
    if (event.persisted) starting = null;
  }}
/>

{#snippet stepNumber(n: number, done: boolean, label: string)}
  <span class="flex items-center gap-2.5 {done || n === step ? 'font-bold text-ink' : ''}">
    <span
      class="grid size-7 flex-none place-items-center rounded-full text-[0.8125rem] font-extrabold {done
        ? 'bg-success-bg text-success'
        : n === step
          ? 'bg-action text-action-on'
          : 'bg-sunken text-ink-2'}"
    >
      {#if done}<Check class="size-4" aria-hidden="true" />{:else}{n}{/if}
    </span>
    {label}
  </span>
{/snippet}

<AuthLayout>
  <header class="flex flex-col gap-4">
    <h1 class="text-display">Set up this server</h1>
    <p class="text-small text-ink-2">
      This server has no accounts yet. The first person to sign in with the setup token becomes its
      administrator.
    </p>
    <ol
      class="flex flex-col gap-3 text-small text-ink-2 sm:flex-row sm:gap-6"
      aria-label="Setup steps"
    >
      <li aria-current={step === 1 ? 'step' : undefined}>
        {@render stepNumber(1, step === 2, 'Setup token')}
      </li>
      <li aria-current={step === 2 ? 'step' : undefined}>
        {@render stepNumber(2, false, 'Sign in as first admin')}
      </li>
    </ol>
  </header>

  {#if failure}
    <Banner variant={failure.variant}>
      <strong class="font-bold">{failure.title}</strong>
      {failure.text}
    </Banner>
  {/if}
  {#if startFailed}
    <Banner variant="danger">
      <strong class="font-bold">Sign-in could not start.</strong> Try again. If it keeps happening, ask
      your administrator.
    </Banner>
  {/if}

  {#if loadFailed}
    <Banner variant="danger">
      <strong class="font-bold">Sign-in options could not be loaded.</strong>
      {#snippet action()}
        <Button size="sm" onclick={load}>Retry</Button>
      {/snippet}
    </Banner>
  {:else if providers === null}
    <div class="flex flex-col gap-3" aria-busy="true" aria-label="Loading">
      <Skeleton class="h-10 w-full" />
      <Skeleton class="h-12 w-full" />
    </div>
  {:else if step === 1}
    <form class="flex flex-col gap-5" onsubmit={next}>
      <Field
        label="Setup token"
        bind:value={token}
        mono
        autocomplete="off"
        spellcheck={false}
        placeholder="Paste the token"
        error={showTokenError
          ? 'This setup token is not valid. A token works once and changes when the server restarts.'
          : undefined}
        helper="The server prints it once in its log at first start. To print a new one, run this on the server:"
      >
        <div class="flex items-start gap-2 rounded-control bg-sunken py-2 pr-2 pl-3 text-ink">
          <code class="min-w-0 flex-1 pt-1.5 font-code text-code break-words">{SETUP_COMMAND}</code>
          <CopyButton text={SETUP_COMMAND} label="Copy command" />
        </div>
      </Field>
      <Button
        type="submit"
        variant="primary"
        size="lg"
        block
        disabled={trimmed === ''}
        disabledReason="Paste the setup token first."
      >
        Continue
      </Button>
    </form>
  {:else}
    <p class="flex items-center gap-2 text-small text-ink-2">
      <Info class="size-4 flex-none" aria-hidden="true" />Now sign in with the account that will be
      the administrator.
    </p>
    <ProviderButtons providers={providers.providers} {starting} onpick={start} />
  {/if}

  <p class="text-small text-ink-2">
    The token is valid only while the server has no accounts. Later users join by invite.
  </p>
</AuthLayout>
