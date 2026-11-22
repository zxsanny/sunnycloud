<script lang="ts">
  import { onMount } from 'svelte';
  import {
    api,
    type Account,
    type ApiError,
    type DeviceList,
    type ProviderList,
    type Quota,
    type SessionList,
  } from '$lib/api';
  import { navigate, route } from '$lib/router';
  import { session } from '$lib/session';
  import { Banner, Button } from '$lib/ui';
  import AppShell from './AppShell.svelte';
  import BrowserSessionsSection from './account/BrowserSessionsSection.svelte';
  import DevicesSection from './account/DevicesSection.svelte';
  import LeaveSection from './account/LeaveSection.svelte';
  import ProfileCard from './account/ProfileCard.svelte';
  import StorageCard from './account/StorageCard.svelte';
  import { deadlineText, linkOutcome, type DeviceRow, type LinkOutcome } from './account/view';

  // Each section loads and fails on its own, so one broken list never blanks the others.
  let account = $state<Account | null>(null);
  let accountError = $state<ApiError | null>(null);
  let quota = $state<Quota | null>(null);
  let quotaError = $state<ApiError | null>(null);
  let devices = $state<DeviceList | null>(null);
  let devicesError = $state<ApiError | null>(null);
  let sessions = $state<SessionList | null>(null);
  let sessionsError = $state<ApiError | null>(null);
  let linkNotice = $state<LinkOutcome | null>(null);
  let providers = $state<ProviderList | null>(null);
  let providersError = $state<ApiError | null>(null);
  let deadline = $state<string | null>(null);

  async function loadAccount() {
    accountError = null;
    const result = await api.getAccountsMe();
    if (result.ok) account = result.value;
    else accountError = result.error;
  }

  async function loadQuota() {
    quotaError = null;
    const result = await api.getAccountsMeQuota();
    if (result.ok) quota = result.value;
    else quotaError = result.error;
  }

  async function loadDevices() {
    devicesError = null;
    const result = await api.getDevices();
    if (result.ok) devices = result.value;
    else devicesError = result.error;
  }

  async function loadSessions() {
    sessionsError = null;
    const result = await api.getSessions();
    if (result.ok) sessions = result.value;
    else sessionsError = result.error;
  }

  async function loadProviders() {
    providersError = null;
    const result = await api.getAuthProviders();
    if (result.ok) providers = result.value;
    else providersError = result.error;
  }

  function revoked(row: DeviceRow) {
    if (row.kind === 'desktop') {
      if (devices) devices = { items: devices.items.filter((device) => device.id !== row.id) };
    } else if (sessions) {
      sessions = { items: sessions.items.filter((item) => item.id !== row.id) };
    }
  }

  /** The server revoked every session of the account with the request, so no call is needed to sign out. */
  function deleted(at: string) {
    deadline = at;
    account = null;
    quota = null;
    devices = null;
    sessions = null;
    session.account = null;
    session.status = 'signed-out';
  }

  onMount(() => {
    // The provider callback ends a link with a redirect to /account?linked=1 or ?error=<reason>: show it once and
    // take it out of the address, so a reload does not show it again.
    linkNotice = linkOutcome(route.query);
    if (linkNotice !== null) navigate('/account', { replace: true });
    void Promise.all([loadAccount(), loadQuota(), loadDevices(), loadSessions(), loadProviders()]);
  });
</script>

<AppShell>
  {#if deadline !== null}
    <Banner variant="success">
      <strong class="font-bold">Account deletion requested.</strong>
      Your files, links, devices and sessions are removed by {deadlineText(deadline)}. You are
      signed out.
      {#snippet action()}
        <Button size="sm" onclick={() => navigate('/login')}>Go to sign-in</Button>
      {/snippet}
    </Banner>
  {:else}
    <div>
      <h1 class="text-h1">Account</h1>
      <p class="text-small text-ink-2">
        Your profile, your storage, and the apps signed in as you.
      </p>
    </div>
    {#if linkNotice}
      <Banner variant={linkNotice.variant}>
        <strong class="font-bold">{linkNotice.title}</strong>
        {linkNotice.text}
        {#snippet action()}
          <Button size="sm" onclick={() => (linkNotice = null)}>Dismiss</Button>
        {/snippet}
      </Banner>
    {/if}
    <div class="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_340px]">
      <div class="flex flex-col gap-6">
        <ProfileCard
          {account}
          error={accountError}
          {providers}
          {providersError}
          onretry={loadAccount}
          onretryproviders={loadProviders}
        />
        <DevicesSection
          {devices}
          error={devicesError}
          {sessions}
          {sessionsError}
          onretry={loadDevices}
          onretrysessions={loadSessions}
          onrevoked={revoked}
        />
        <BrowserSessionsSection
          {sessions}
          error={sessionsError}
          onretry={loadSessions}
          onrevoked={revoked}
        />
      </div>
      <div class="flex flex-col gap-6">
        <StorageCard {quota} error={quotaError} onretry={loadQuota} />
        <LeaveSection accountId={account?.id ?? null} ondeleted={deleted} />
      </div>
    </div>
  {/if}
</AppShell>
