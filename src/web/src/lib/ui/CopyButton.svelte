<script lang="ts">
  import Check from '@lucide/svelte/icons/check';
  import Copy from '@lucide/svelte/icons/copy';
  import Button from './Button.svelte';

  let { text, label }: { text: string; label: string } = $props();

  let state = $state<'idle' | 'copied' | 'failed'>('idle');

  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      state = 'copied';
    } catch (cause) {
      console.warn('The text could not be copied to the clipboard.', cause);
      state = 'failed';
    }
  }

  $effect(() => {
    if (state === 'idle') return;
    const timer = setTimeout(() => (state = 'idle'), 2000);
    return () => clearTimeout(timer);
  });
</script>

<Button variant="ghost" size="sm" aria-label={label} onclick={copy}>
  {#if state === 'copied'}
    <Check class="size-4" aria-hidden="true" />Copied
  {:else if state === 'failed'}
    Copy failed
  {:else}
    <Copy class="size-4" aria-hidden="true" />Copy
  {/if}
</Button>
