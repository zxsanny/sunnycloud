<script lang="ts">
  import { BrandMark, EmptyState } from '$lib/ui';
</script>

<main class="flex min-h-screen flex-col items-center justify-center gap-6 bg-surface p-6">
  <BrandMark />
  <EmptyState title="Page not found">
    This address does not lead anywhere. <a class="text-link underline" href="/">Go to your files</a
    >.
  </EmptyState>
</main>
