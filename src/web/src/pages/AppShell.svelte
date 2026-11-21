<script lang="ts">
  import LogOut from '@lucide/svelte/icons/log-out';
  import type { Snippet } from 'svelte';
  import { navigate } from '$lib/router';
  import { session } from '$lib/session';
  import { BrandMark, Button, ThemeToggle } from '$lib/ui';
  import { realtime } from '../app/realtime';
  import ConnectionChip from './ConnectionChip.svelte';

  let { children }: { children: Snippet } = $props();

  let signingOut = $state(false);

  async function signOut() {
    signingOut = true;
    const done = await session.signOut();
    signingOut = false;
    if (done) navigate('/login');
  }
</script>

<div class="flex min-h-screen flex-col bg-page">
  <header class="flex h-16 flex-none items-center gap-3 px-4 lg:px-8">
    <a href="/" class="rounded-control text-ink no-underline"><BrandMark /></a>
    <div class="flex-1"></div>
    <ConnectionChip state={realtime.state} />
    <ThemeToggle />
    {#if session.account}
      <span class="hidden text-row text-ink-2 md:inline">{session.account.display_name}</span>
    {/if}
    <Button variant="ghost" size="sm" loading={signingOut} onclick={signOut}>
      <LogOut class="size-4" aria-hidden="true" />Sign out
    </Button>
  </header>
  <main class="flex-1 rounded-tl-card border border-line bg-surface">
    <div class="mx-auto flex w-full max-w-[1440px] flex-col gap-5 px-4 py-6 lg:px-8">
      {@render children()}
    </div>
  </main>
</div>
