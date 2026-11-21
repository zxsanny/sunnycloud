<script lang="ts">
  import { onMount } from 'svelte';
  import {
    api,
    type Account,
    type ApiError,
    type DeviceList,
    type ProviderList,
    type Quota,
  } from '$lib/api';
  import { navigate } from '$lib/router';
  import { session } from '$lib/session';
  import { Banner, Button } from '$lib/ui';
  import AppShell from './AppShell.svelte';
  import DevicesSection from './account/DevicesSection.svelte';
  import LeaveSection from './account/LeaveSection.svelte';
  import ProfileCard from './account/ProfileCard.svelte';
  import StorageCard from './account/StorageCard.svelte';
  import { deadlineText } from './account/view';

  // Each section loads and fails on its own, so one broken list never blanks the others.
  let account = $state<Account | null>(null);
  let accountError = $state<ApiError | null>(null);
  let quota = $state<Quota | null>(null);
  let quotaError = $state<ApiError | null>(null);
  let devices = $state<DeviceList | null>(null);
  let devicesError = $state<ApiError | null>(null);
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

  async function loadProviders() {
    providersError = null;
    const result = await api.getAuthProviders();
    if (result.ok) providers = result.value;
    else providersError = result.error;
  }

  function revoked(id: string) {
    if (devices) devices = { items: devices.items.filter((device) => device.id !== id) };
  }

  /** The server revoked every session of the account with the request, so no call is needed to sign out. */
  function deleted(at: string) {
    deadline = at;
    account = null;
    quota = null;
    devices = null;
    session.account = null;
    session.status = 'signed-out';
  }

  onMount(() => {
    void Promise.all([loadAccount(), loadQuota(), loadDevices(), loadProviders()]);
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
        <DevicesSection {devices} error={devicesError} onretry={loadDevices} onrevoked={revoked} />
      </div>
      <div class="flex flex-col gap-6">
        <StorageCard {quota} error={quotaError} onretry={loadQuota} />
        <LeaveSection accountId={account?.id ?? null} ondeleted={deleted} />
      </div>
    </div>
  {/if}
</AppShell>
