<script lang="ts">
  import type { Snippet } from 'svelte';
  import { BrandMark, ThemeToggle } from '$lib/ui';

  let { children }: { children: Snippet } = $props();
  const host = globalThis.location.host;
</script>

<div class="grid min-h-screen lg:grid-cols-[minmax(0,1.05fr)_minmax(0,1fr)]">
  <aside
    class="hidden flex-col gap-8 bg-action-subtle p-10 text-action-on-subtle lg:flex xl:p-14"
    aria-label="About this server"
  >
    <BrandMark />
    <div class="mx-auto flex w-full max-w-[520px] flex-1 flex-col justify-center gap-10">
      <svg class="w-full max-w-[440px]" viewBox="0 0 440 300" aria-hidden="true">
        <g class="stroke-sun" stroke-width="6" stroke-linecap="round">
          <path
            d="M268 22v20M268 182v20M336 112h20M180 112h20M316 64l14-14M206 174l14-14M316 160l14 14M206 50l14 14"
          />
        </g>
        <circle class="fill-sun" cx="268" cy="112" r="58" />
        <path
          class="fill-[light-dark(var(--g-25),var(--g-300))]"
          d="M318 268a26 26 0 0 1 2-52 34 34 0 0 1 64 8 22 22 0 0 1 6 44z"
        />
        <path
          class="fill-action"
          d="M150 236a34 34 0 0 1-3-68 46 46 0 0 1 87-6 32 32 0 0 1 10 74z"
        />
        <path
          class="fill-[light-dark(var(--g-0),var(--g-150))] stroke-action"
          stroke-width="4"
          d="M108 286a40 40 0 0 1-4-80 54 54 0 0 1 102-8 38 38 0 0 1 12 88z"
        />
      </svg>
      <div class="flex flex-col gap-2">
        <p class="font-code text-[0.9375rem] font-semibold">{host}</p>
        <p class="max-w-md text-h2">Your files and photos, kept on your own server.</p>
      </div>
    </div>
  </aside>

  <main class="flex flex-col items-center bg-surface px-4 py-10 md:px-6 lg:justify-center lg:py-16">
    <div class="flex w-full max-w-[400px] flex-col gap-6">
      <div class="flex items-start justify-between gap-3">
        <div class="flex flex-col gap-1 lg:hidden">
          <BrandMark />
          <span class="font-code text-small text-ink-2">{host}</span>
        </div>
        <div class="ml-auto"><ThemeToggle /></div>
      </div>
      {@render children()}
    </div>
  </main>
</div>
