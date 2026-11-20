<script lang="ts">
  import Mail from '@lucide/svelte/icons/mail';
  import { onMount } from 'svelte';
  import { api, isAppLevelError, type ProviderList } from '$lib/api';
  import { beginLogin, describeLoginError, safeReturnPath } from '$lib/auth';
  import { followExternal, navigate, route } from '$lib/router';
  import { session } from '$lib/session';
  import { Banner, Button, Skeleton } from '$lib/ui';
  import AuthLayout from './AuthLayout.svelte';
  import ProviderButtons from './ProviderButtons.svelte';

  const returnTo = $derived(safeReturnPath(route.query.get('return')));
  const failure = $derived(describeLoginError(route.query.get('error')));
  const inviteToken = $derived(
    route.path === '/invite' && route.hash.length > 1 ? route.hash.slice(1) : null,
  );

  let providers = $state<ProviderList | null>(null);
  let loadFailed = $state(false);
  let starting = $state<string | null>(null);
  let startFailed = $state(false);

  async function load() {
    loadFailed = false;
    const [list] = await Promise.all([api.getAuthProviders(), session.load()]);
    if (!list.ok) {
      loadFailed = true;
      return;
    }
    if (list.value.setup_required) {
      navigate(`/setup${route.search}`, { replace: true });
      return;
    }
    providers = list.value;
    if (session.status === 'ready') navigate(returnTo, { replace: true });
  }

  async function start(provider: string) {
    starting = provider;
    startFailed = false;
    const outcome = await beginLogin({ provider, returnTo, inviteToken });
    if (outcome.ok) {
      followExternal(outcome.authorizationUrl);
      return;
    }
    starting = null;
    // Rate limits and server faults already raised an app-level notice.
    startFailed = outcome.error === null || !isAppLevelError(outcome.error);
  }

  onMount(load);
</script>

<svelte:window
  onpageshow={(event) => {
    if (event.persisted) starting = null;
  }}
/>

<AuthLayout>
  <header class="flex flex-col gap-2">
    <h1 class="text-display">Sign in to sunnycloud</h1>
    <p class="text-small text-ink-2">Use the account your administrator set up for you.</p>
  </header>

  {#if inviteToken && !failure}
    <Banner variant="info" icon={Mail}>
      <strong class="font-bold">You have an invite.</strong> Choose how to sign in. Your account is created
      on the first sign-in; the invite works once.
    </Banner>
  {/if}
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
    <div class="flex flex-col gap-3" aria-busy="true" aria-label="Loading sign-in options">
      <Skeleton class="h-12 w-full" />
      <Skeleton class="h-12 w-full" />
    </div>
  {:else if providers.providers.length === 0}
    <Banner variant="warning">
      <strong class="font-bold">No sign-in options are enabled.</strong> Ask your administrator to turn
      one on.
    </Banner>
  {:else}
    <ProviderButtons providers={providers.providers} {starting} onpick={start} />
  {/if}

  <hr class="border-line" />
  <p class="text-small text-ink-2">
    <strong class="font-bold text-ink">New here?</strong> You need an invite from your administrator.
  </p>
</AuthLayout>
