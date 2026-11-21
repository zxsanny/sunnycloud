<script lang="ts">
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import Copy from '@lucide/svelte/icons/copy';
  import type { CreatedShare } from '$lib/api';
  import { formatDate } from '$lib/format';
  import { Button } from '$lib/ui';

  let { created }: { created: { share: CreatedShare; url: string } } = $props();

  let copy = $state<'idle' | 'copied' | 'failed'>('idle');

  async function copyLink() {
    try {
      await navigator.clipboard.writeText(created.url);
      copy = 'copied';
    } catch (cause) {
      console.warn('The link could not be copied to the clipboard.', cause);
      copy = 'failed';
    }
  }
</script>

<section
  class="flex flex-col gap-3 rounded-card bg-action-subtle p-4 text-action-on-subtle"
  aria-labelledby="created-h"
>
  <h3 id="created-h" class="flex items-center gap-2 text-h3">
    <CircleCheck class="size-5" aria-hidden="true" />Link created
  </h3>
  <p class="font-code text-code break-all select-all" id="share-url">{created.url}</p>
  <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
    <Button variant="primary" aria-describedby="share-url" onclick={copyLink}>
      <Copy class="size-4" aria-hidden="true" />Copy link
    </Button>
    <p class="text-small font-semibold">This is the only time the full link is shown.</p>
  </div>
  <p class="text-small">
    Expires {formatDate(created.share.expires_at)}{created.share.has_password
      ? ' · Password required'
      : ''}
  </p>
  {#if copy === 'copied'}
    <p class="flex items-center gap-2 text-small font-semibold" role="status">
      <CircleCheck class="size-4" aria-hidden="true" />Link copied
    </p>
  {:else if copy === 'failed'}
    <p class="text-small font-semibold" role="alert">
      The link could not be copied. Select it above and copy it by hand.
    </p>
  {/if}
</section>
