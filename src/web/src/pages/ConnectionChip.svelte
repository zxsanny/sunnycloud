<script lang="ts">
  import type { ConnectionState } from '$lib/realtime';

  let { state }: { state: ConnectionState } = $props();

  // connecting and reconnecting look alike to the user: the page is not live right now.
  const view = $derived(
    state === 'live'
      ? { label: 'Live', tone: 'bg-success-bg text-success', dot: 'bg-success' }
      : state === 'connecting' || state === 'reconnecting'
        ? { label: 'Reconnecting', tone: 'bg-warning-bg text-warning-on', dot: 'bg-warning' }
        : { label: 'Offline', tone: 'bg-danger-bg text-danger-on', dot: 'bg-danger' },
  );
</script>

{#if state !== 'idle'}
  <span
    role="status"
    aria-label="Live updates: {view.label}"
    class="inline-flex h-6 items-center gap-1.5 rounded-full px-2.5 text-label uppercase {view.tone}"
  >
    <span class="size-2 rounded-full {view.dot}" aria-hidden="true"></span>{view.label}
  </span>
{/if}
