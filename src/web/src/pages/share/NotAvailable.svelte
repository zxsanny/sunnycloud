<script lang="ts">
  import Link from '@lucide/svelte/icons/link';
</script>

<!-- The one page for an unknown, expired and revoked link: it takes no input, so no cause can reach it. -->
<section
  class="mx-auto flex w-full max-w-[520px] flex-col items-center gap-4 rounded-card border border-line bg-raised px-6 py-10 text-center shadow-card sm:px-10"
  aria-labelledby="na-h"
>
  <span class="grid size-14 place-items-center rounded-full bg-sunken text-ink-2">
    <Link class="size-7" aria-hidden="true" />
  </span>
  <h1 id="na-h" class="text-h1">This link isn't available</h1>
  <p class="text-ink-2">Ask the person who sent it for a new link.</p>
</section>
