<script lang="ts">
  import { onMount } from 'svelte';
  import { navigate, route } from '$lib/router';
  import { session } from '$lib/session';
  import { Skeleton } from '$lib/ui';
  import AppShell from './AppShell.svelte';
  import NotFoundPage from './NotFoundPage.svelte';
  import HealthTab from './admin/HealthTab.svelte';
  import InvitesTab from './admin/InvitesTab.svelte';
  import QuotasTab from './admin/QuotasTab.svelte';
  import UsersTab from './admin/UsersTab.svelte';
  import { isAdmin, TAB_LABELS, TABS, tabOf } from './admin/view';

  const tab = $derived(tabOf(route.query.get('tab')));

  // Only a session read tells the role; the server is the real gate and answers 403 to anyone else.
  onMount(() => {
    if (session.status !== 'ready') void session.load();
  });
</script>

{#if session.status === 'ready' && !isAdmin(session.account)}
  <NotFoundPage />
{:else}
  <AppShell>
    <div>
      <h1 class="text-h1">Admin</h1>
      <p class="text-small text-ink-2">
        Who can use this server, how much each person can store, and how the server is doing.
      </p>
    </div>
    {#if session.status === 'ready'}
      <div class="flex gap-1 border-b border-line" role="tablist" aria-label="Admin sections">
        {#each TABS as name (name)}
          <button
            type="button"
            role="tab"
            id="{name}-tab"
            aria-controls="{name}-panel"
            aria-selected={tab === name}
            class="h-10 rounded-t-control px-3 text-row font-[650] {tab === name
              ? 'border-b-2 border-action text-ink'
              : 'text-ink-2 hover:bg-hover'}"
            onclick={() => navigate(`/admin?tab=${name}`)}
          >
            {TAB_LABELS[name]}
          </button>
        {/each}
      </div>
      {#if tab === 'users'}
        <UsersTab />
      {:else if tab === 'invites'}
        <InvitesTab />
      {:else if tab === 'quotas'}
        <QuotasTab />
      {:else}
        <HealthTab />
      {/if}
    {:else if session.status === 'failed'}
      <p class="text-row text-ink-2" role="alert">
        Your session could not be checked. Reload the page to try again.
      </p>
    {:else}
      <Skeleton class="h-40 w-full" />
    {/if}
  </AppShell>
{/if}
