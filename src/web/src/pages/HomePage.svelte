<script lang="ts">
  import { onMount } from 'svelte';
  import { formatBytes } from '$lib/format';
  import { session } from '$lib/session';
  import { Banner, Button, EmptyState, Skeleton } from '$lib/ui';
  import AppShell from './AppShell.svelte';

  const account = $derived(session.account);

  onMount(() => session.load());
</script>

<AppShell>
  <h1 class="text-h1">Files</h1>

  {#if session.status === 'failed'}
    <Banner variant="danger">
      <strong class="font-bold">Your account could not be loaded.</strong>
      {#snippet action()}
        <Button size="sm" onclick={() => session.load()}>Retry</Button>
      {/snippet}
    </Banner>
  {:else if account === null}
    <div class="flex flex-col gap-3" aria-busy="true" aria-label="Loading your account">
      <Skeleton class="h-6 w-1/3" />
      <Skeleton class="h-4 w-1/4" />
    </div>
  {:else}
    <section
      class="flex flex-col gap-2 rounded-card border border-line bg-raised p-4 shadow-card"
      aria-labelledby="account-h"
    >
      <h2 id="account-h" class="text-label text-ink-2 uppercase">Signed in as</h2>
      <p class="flex items-center gap-2 text-h3">
        {account.display_name}
        {#if account.role === 'admin'}
          <span
            class="inline-flex h-6 items-center rounded-full bg-sun-fill px-2.5 text-label text-sun-on uppercase"
            >Admin</span
          >
        {/if}
      </p>
      <ul class="text-small text-ink-2">
        {#each account.identities as identity (identity.provider + (identity.email ?? ''))}
          <li>{identity.provider}{identity.email ? `: ${identity.email}` : ''}</li>
        {/each}
      </ul>
      <p class="text-small text-ink-2">
        Storage: {formatBytes(account.quota.used)} of {formatBytes(account.quota.effective_limit)}
      </p>
    </section>

    <div class="rounded-card border border-line bg-raised">
      <EmptyState title="Your files will appear here">
        The file browser is not part of this version yet.
      </EmptyState>
    </div>
  {/if}
</AppShell>
