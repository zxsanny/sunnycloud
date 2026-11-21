<script lang="ts">
  import { api, type ApiError, type ProviderList } from '$lib/api';
  import { isFollowableUrl } from '$lib/auth';
  import { followExternal } from '$lib/router';
  import { Banner, Button, Skeleton } from '$lib/ui';
  import SectionError from './SectionError.svelte';
  import { errorText } from './view';

  interface Props {
    providers: ProviderList | null;
    providersError: ApiError | null;
    /** Provider keys this account already has. */
    linked: string[];
    onretry: () => void;
  }

  let { providers, providersError, linked, onretry }: Props = $props();

  let starting = $state<string | null>(null);
  let failure = $state<ApiError | null>(null);
  let refused = $state(false);

  const candidates = $derived(
    providers?.providers.filter((provider) => !linked.includes(provider.key)) ?? [],
  );

  /** The link attempt is one call; the provider's own page does the rest. The page never builds the URL. */
  async function start(key: string) {
    starting = key;
    failure = null;
    refused = false;
    const result = await api.postIdentityLinkAttempt({ provider: key });
    if (result.ok && isFollowableUrl(result.value.authorization_url)) {
      followExternal(result.value.authorization_url);
      return;
    }
    starting = null;
    if (result.ok) refused = true;
    else failure = result.error;
  }
</script>

<div
  class="flex flex-col gap-3 rounded-card border border-line bg-raised p-4"
  aria-label="Link another provider"
  role="group"
>
  {#if providersError}
    <SectionError what="Sign-in options could not be loaded." error={providersError} {onretry} />
  {:else if providers === null}
    <Skeleton class="h-10 w-full" />
  {:else if candidates.length === 0}
    <p class="text-small text-ink-2">
      Every sign-in option your administrator enabled is already linked.
    </p>
  {:else}
    <p class="text-small text-ink-2">
      You sign in to the other provider and approve the link there. It is never matched by email
      address.
    </p>
    <div class="flex flex-wrap gap-2">
      {#each candidates as provider (provider.key)}
        <Button
          loading={starting === provider.key}
          disabled={starting !== null && starting !== provider.key}
          onclick={() => start(provider.key)}
        >
          Link {provider.display_name}
        </Button>
      {/each}
    </div>
  {/if}
  {#if failure}
    <Banner variant="danger">
      <strong class="font-bold">The provider could not be linked.</strong>
      {errorText(failure)}
      {#if failure.requestId}
        <span class="block text-small"
          >Reference: <code class="font-code">{failure.requestId}</code></span
        >
      {/if}
    </Banner>
  {/if}
  {#if refused}
    <Banner variant="danger">
      <strong class="font-bold">The provider could not be linked.</strong> The server sent an address
      the app does not follow.
    </Banner>
  {/if}
</div>
