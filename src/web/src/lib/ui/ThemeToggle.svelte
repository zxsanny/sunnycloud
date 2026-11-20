<script lang="ts">
  import Monitor from '@lucide/svelte/icons/monitor';
  import Moon from '@lucide/svelte/icons/moon';
  import Sun from '@lucide/svelte/icons/sun';
  import { theme, type ThemeChoice } from '$lib/theme';

  const choices: { value: ThemeChoice; label: string; icon: typeof Sun }[] = [
    { value: 'system', label: 'Match the system theme', icon: Monitor },
    { value: 'light', label: 'Light theme', icon: Sun },
    { value: 'dark', label: 'Dark theme', icon: Moon },
  ];
</script>

<div
  class="inline-flex rounded-control border border-line bg-raised p-0.5"
  role="group"
  aria-label="Theme"
>
  {#each choices as { value, label, icon: Icon } (value)}
    <button
      type="button"
      aria-label={label}
      aria-pressed={theme.choice === value}
      title={label}
      onclick={() => theme.set(value)}
      class="grid size-8 place-items-center rounded-control {theme.choice === value
        ? 'bg-action-subtle text-action-on-subtle'
        : 'text-ink-2 hover:bg-hover'}"
    >
      <Icon class="size-4" aria-hidden="true" />
    </button>
  {/each}
</div>
