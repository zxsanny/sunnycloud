<script lang="ts">
  import type { ApiError } from '$lib/api';
  import { Banner, Button } from '$lib/ui';
  import { errorText } from './view';

  interface Props {
    /** What failed, as the section's own words: "The devices could not be loaded." */
    what: string;
    error: ApiError;
    onretry?: () => void;
  }

  let { what, error, onretry }: Props = $props();
</script>

<Banner variant="danger">
  <strong class="font-bold">{what}</strong>
  {errorText(error)}
  {#if error.requestId}
    <span class="block text-small">Reference: <code class="font-code">{error.requestId}</code></span
    >
  {/if}
  {#snippet action()}
    {#if onretry}
      <Button size="sm" onclick={onretry}>Retry</Button>
    {/if}
  {/snippet}
</Banner>
