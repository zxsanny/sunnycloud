<script lang="ts">
  import { onMount } from 'svelte';
  import type { DeviceGrantApproval, DeviceGrantDecision } from '$lib/api';
  import { session } from '$lib/session';
  import { BrandMark, Skeleton, ThemeToggle } from '$lib/ui';
  import CodeEntry from './deviceApproval/CodeEntry.svelte';
  import Outcome from './deviceApproval/Outcome.svelte';
  import Review from './deviceApproval/Review.svelte';

  type Phase =
    | { name: 'enter' }
    | { name: 'review'; approval: DeviceGrantApproval; code: string }
    | { name: 'approved' | 'denied'; device: string }
    | { name: 'expired' };

  let phase = $state<Phase>({ name: 'enter' });

  function decided(approval: DeviceGrantApproval, decision: DeviceGrantDecision) {
    phase = { name: decision === 'approve' ? 'approved' : 'denied', device: approval.device };
  }

  // No session: the first read answers 401 and the app sends the visitor to sign-in with this page as the return path.
  onMount(() => {
    if (session.status !== 'ready') void session.load();
  });
</script>

<div class="min-h-screen bg-page">
  <header class="border-b border-line bg-surface">
    <div class="mx-auto flex h-16 max-w-[960px] items-center justify-between gap-3 px-4">
      <BrandMark />
      <div class="flex min-w-0 items-center gap-2">
        {#if session.account}
          <span class="truncate text-small font-semibold" data-testid="signed-in-as"
            >Signed in as {session.account.display_name}</span
          >
        {/if}
        <ThemeToggle />
      </div>
    </div>
  </header>
  <main class="mx-auto max-w-[560px] px-4 py-6 sm:py-12">
    {#if session.status === 'failed'}
      <p class="text-row text-ink-2" role="alert">
        Your session could not be checked. Reload the page to try again.
      </p>
    {:else if session.status !== 'ready'}
      <Skeleton class="h-80 w-full" />
    {:else if phase.name === 'enter'}
      <CodeEntry
        onfound={(approval, code) => (phase = { name: 'review', approval, code })}
        oninvalid={() => (phase = { name: 'expired' })}
      />
    {:else if phase.name === 'review'}
      {@const approval = phase.approval}
      <Review
        {approval}
        code={phase.code}
        ondecided={(decision) => decided(approval, decision)}
        onexpired={() => (phase = { name: 'expired' })}
      />
    {:else if phase.name === 'expired'}
      <Outcome kind="expired" onretry={() => (phase = { name: 'enter' })} />
    {:else}
      <Outcome kind={phase.name} device={phase.device} />
    {/if}
  </main>
</div>
