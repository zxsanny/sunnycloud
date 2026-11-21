<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import { route } from '$lib/router';
  import { PublicShareState, removeTokenFromAddress, tokenOfPath } from '$lib/shares';
  import { BrandMark, Skeleton, ThemeToggle } from '$lib/ui';
  import LimitedCard from './share/LimitedCard.svelte';
  import NotAvailable from './share/NotAvailable.svelte';
  import PasswordGate from './share/PasswordGate.svelte';
  import ShareView from './share/ShareView.svelte';

  // The token lives in this component for the life of the page; once the first answer is in, the address and the
  // history no longer hold it (a reload therefore opens the "not available" page).
  const token = tokenOfPath(route.path);
  const share = new PublicShareState(api, token);

  onMount(() => {
    void share.open().then(removeTokenFromAddress);
  });
</script>

<svelte:head>
  <!-- From mount on, no request of this page names the page address as its referrer, whatever it still held. -->
  <meta name="referrer" content="no-referrer" />
</svelte:head>

<div class="min-h-screen bg-page">
  <header class="border-b border-line bg-surface">
    <div class="mx-auto flex h-16 max-w-[760px] items-center justify-between px-4">
      <BrandMark />
      <ThemeToggle />
    </div>
  </header>
  <main class="mx-auto flex max-w-[760px] flex-col gap-6 px-4 py-6 sm:py-10">
    {#if share.phase === 'loading'}
      <Skeleton class="h-64 w-full" />
    {:else if share.phase === 'available' && share.share && token !== null}
      <ShareView share={share.share} {token} />
    {:else if share.phase === 'password'}
      <PasswordGate {share} />
    {:else if share.phase === 'limited'}
      <LimitedCard {share} />
    {:else}
      <NotAvailable />
    {/if}
  </main>
</div>
