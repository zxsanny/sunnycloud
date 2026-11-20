<script lang="ts">
  import Info from '@lucide/svelte/icons/info';
  import { Button } from '$lib/ui';

  interface Props {
    providers: { key: string; display_name: string }[];
    /** Key of the provider whose sign-in is being started. */
    starting: string | null;
    onpick: (key: string) => void;
  }

  let { providers, starting, onpick }: Props = $props();
</script>

<section class="flex flex-col gap-3" aria-label="Sign-in options">
  {#each providers as provider (provider.key)}
    <Button
      variant="secondary"
      size="lg"
      block
      class="justify-start gap-3.5"
      loading={starting === provider.key}
      disabled={starting !== null && starting !== provider.key}
      onclick={() => onpick(provider.key)}
    >
      <span
        class="grid size-[26px] flex-none place-items-center rounded-control bg-sunken text-[0.75rem] font-extrabold text-ink"
        aria-hidden="true">{provider.display_name.slice(0, 1).toUpperCase()}</span
      >Continue with {provider.display_name}
    </Button>
  {/each}
  <p class="mt-1 flex items-start gap-2 text-small text-ink-2">
    <Info class="mt-0.5 size-4 flex-none" aria-hidden="true" />Your administrator chose these
    sign-in options.
  </p>
</section>
