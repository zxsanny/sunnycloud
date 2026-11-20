<script lang="ts">
  let { class: extra = '' }: { class?: string } = $props();
</script>

<span
  class="inline-flex items-center gap-2.5 text-[1.125rem] font-extrabold tracking-tight {extra}"
>
  <svg class="size-[30px] flex-none" viewBox="0 0 32 32" aria-hidden="true">
    <circle class="fill-sun" cx="20.5" cy="11.5" r="7" />
    <path
      class="fill-action"
      d="M9 26a6 6 0 0 1-.7-11.96A8 8 0 0 1 23.6 15.6 5.2 5.2 0 0 1 23 26z"
    />
  </svg>
  sunnycloud
</span>
