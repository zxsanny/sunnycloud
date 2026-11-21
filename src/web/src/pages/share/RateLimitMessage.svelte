<script lang="ts">
  import Clock from '@lucide/svelte/icons/clock';
  import { Banner } from '$lib/ui';
  import { secondsUntil } from '../deviceApproval/view';

  interface Props {
    /** Epoch milliseconds at which the server's limit ends (from its Retry-After). */
    until: number;
    onend: () => void;
  }

  let { until, onend }: Props = $props();

  let now = $state(Date.now());
  const left = $derived(secondsUntil(until, now));

  // A local display of the server's Retry-After; it asks the server nothing.
  $effect(() => {
    now = Date.now();
    if (now >= until) {
      onend();
      return;
    }
    const timer = setInterval(() => {
      now = Date.now();
      if (now >= until) onend();
    }, 250);
    return () => clearInterval(timer);
  });
</script>

<Banner variant="warning" icon={Clock}>
  <strong class="font-bold">Too many attempts.</strong> Try again in
  <span class="tabular-nums">{left}</span>
  {left === 1 ? 'second' : 'seconds'}.
</Banner>
