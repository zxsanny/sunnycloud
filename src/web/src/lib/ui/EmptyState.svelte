<script lang="ts">
  import type { Snippet } from 'svelte';

  let { title, children, action }: { title: string; children: Snippet; action?: Snippet } =
    $props();
</script>

<div class="flex flex-col items-center gap-3 px-6 py-12 text-center">
  <svg class="h-[72px] w-24" viewBox="0 0 96 72" aria-hidden="true">
    <circle cx="62" cy="24" r="14" class="fill-sun" />
    <path
      d="M26 62a14 14 0 0 1-1.6-27.9A19 19 0 0 1 60.5 37 12.5 12.5 0 0 1 59 62z"
      class="fill-action-subtle stroke-action"
      stroke-width="2"
    />
  </svg>
  <h2 class="text-h2">{title}</h2>
  <p class="max-w-md text-small text-ink-2">{@render children()}</p>
  {#if action}
    <div class="flex flex-wrap justify-center gap-3">{@render action()}</div>
  {/if}
</div>
