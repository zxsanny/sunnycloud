<script lang="ts">
  import Link from '@lucide/svelte/icons/link';
  import Shield from '@lucide/svelte/icons/shield';
  import type { Account, ApiError, ProviderList } from '$lib/api';
  import { Button, Skeleton } from '$lib/ui';
  import LinkProvider from './LinkProvider.svelte';
  import SectionError from './SectionError.svelte';

  interface Props {
    account: Account | null;
    error: ApiError | null;
    providers: ProviderList | null;
    providersError: ApiError | null;
    onretry: () => void;
    onretryproviders: () => void;
  }

  let { account, error, providers, providersError, onretry, onretryproviders }: Props = $props();

  let linking = $state(false);

  function providerName(key: string): string {
    return providers?.providers.find((provider) => provider.key === key)?.display_name ?? key;
  }
</script>

<section class="flex flex-col gap-3" aria-labelledby="profile-h">
  <h2 id="profile-h" class="text-h2">Profile</h2>
  {#if error}
    <SectionError what="Your profile could not be loaded." {error} {onretry} />
  {:else if account}
    <dl class="rounded-card border border-line bg-raised">
      <div class="grid gap-1 border-b border-line px-4 py-3.5 sm:grid-cols-[160px_minmax(0,1fr)]">
        <dt class="text-small font-[650] text-ink-2">Name</dt>
        <dd class="text-row" data-testid="profile-name">
          {account.display_name}
          <span class="text-small font-normal text-ink-2">· from your sign-in provider</span>
        </dd>
      </div>
      <div class="grid gap-1 border-b border-line px-4 py-3.5 sm:grid-cols-[160px_minmax(0,1fr)]">
        <dt class="text-small font-[650] text-ink-2">Email</dt>
        <dd class="flex flex-col gap-1 text-row">
          {#each account.identities.filter((identity) => identity.email) as identity (identity.provider)}
            <span class="truncate">{identity.email}</span>
          {:else}
            <span class="text-ink-2">No email from your sign-in provider</span>
          {/each}
        </dd>
      </div>
      <div class="grid gap-1 border-b border-line px-4 py-3.5 sm:grid-cols-[160px_minmax(0,1fr)]">
        <dt class="text-small font-[650] text-ink-2">Sign-in</dt>
        <dd class="flex flex-wrap items-center gap-x-4 gap-y-2">
          <ul class="flex flex-wrap items-center gap-x-4 gap-y-2" aria-label="Linked providers">
            {#each account.identities as identity (identity.provider)}
              <li class="flex items-center gap-2 text-row">
                <span
                  class="grid size-6 flex-none place-items-center rounded-control bg-sunken text-[0.6875rem] font-extrabold text-ink"
                  aria-hidden="true"
                  >{providerName(identity.provider).slice(0, 2).toUpperCase()}</span
                >{providerName(identity.provider)}
              </li>
            {/each}
          </ul>
          <Button
            size="sm"
            class="sm:ml-auto"
            aria-expanded={linking}
            onclick={() => (linking = !linking)}
          >
            <Link class="size-4" aria-hidden="true" />Link another provider
          </Button>
        </dd>
      </div>
      <div class="grid gap-1 px-4 py-3.5 sm:grid-cols-[160px_minmax(0,1fr)] sm:items-center">
        <dt class="text-small font-[650] text-ink-2">Role</dt>
        <dd>
          <span
            class="inline-flex items-center gap-1.5 rounded-full bg-sun-fill px-2.5 py-0.5 text-small font-semibold text-sun-on"
          >
            {#if account.role === 'admin'}<Shield class="size-3.5" aria-hidden="true" />{/if}
            {account.role === 'admin' ? 'Admin' : 'Member'}
          </span>
        </dd>
      </div>
    </dl>
    {#if linking}
      <LinkProvider
        {providers}
        {providersError}
        linked={account.identities.map((identity) => identity.provider)}
        onretry={onretryproviders}
      />
    {/if}
  {:else}
    <div
      class="flex flex-col gap-4 rounded-card border border-line bg-raised p-4"
      aria-busy="true"
      aria-label="Loading your profile"
    >
      <Skeleton class="h-3.5 w-36" />
      <Skeleton class="h-3.5 w-56" />
      <Skeleton class="h-3.5 w-28" />
      <Skeleton class="h-5 w-16 rounded-full" />
    </div>
  {/if}
</section>
