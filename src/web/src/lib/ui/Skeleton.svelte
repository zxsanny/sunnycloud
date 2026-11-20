<script lang="ts">
  let { class: extra = '' }: { class?: string } = $props();
</script>

<span
  class="block animate-pulse-soft rounded-control bg-sunken motion-reduce:animate-none {extra}"
  aria-hidden="true"
></span>
