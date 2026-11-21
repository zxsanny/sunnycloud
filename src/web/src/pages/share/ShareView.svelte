<script lang="ts">
  import Download from '@lucide/svelte/icons/download';
  import FileText from '@lucide/svelte/icons/file-text';
  import type { ShareOpen } from '$lib/api';
  import { renderMode } from '$lib/files';
  import { formatBytes, formatDate } from '$lib/format';
  import { shareContentUrl, typeLabel } from '$lib/shares';
  import {
    DOWNLOAD_FAILURE_TEXT,
    HASH_VERIFY_MAX_FF_SAFARI,
    VERIFIED_TEXT,
    transfer,
    unverifiedText,
    type DownloadOutcome,
  } from '$lib/transfer';
  import { Banner, Button } from '$lib/ui';

  let { share, token }: { share: ShareOpen; token: string } = $props();

  let downloading = $state(false);
  let outcome = $state<DownloadOutcome | null>(null);

  // Only the seven allowlisted types are shown by the browser; the server's media type decides, never a file name.
  const inline = $derived(renderMode(share.media_type) === 'inline');
  const isVideo = $derived((share.media_type ?? '').toLowerCase().startsWith('video/'));
  const src = $derived(shareContentUrl(token));
  const details = $derived(
    `${formatBytes(share.size)} · ${typeLabel(share.media_type)} · Available until ${formatDate(share.expires_at)}`,
  );
  const result = $derived(
    outcome === null
      ? null
      : outcome.kind === 'VerifiedSaved'
        ? VERIFIED_TEXT
        : outcome.kind === 'SavedUnverified'
          ? unverifiedText(outcome.reason, HASH_VERIFY_MAX_FF_SAFARI)
          : DOWNLOAD_FAILURE_TEXT[outcome.reason],
  );

  async function download() {
    downloading = true;
    outcome = await transfer.download({ shareToken: token });
    downloading = false;
  }
</script>

<article
  class="mx-auto flex w-full flex-col overflow-hidden rounded-card border border-line bg-raised shadow-card"
  aria-labelledby="file-h"
>
  {#if inline && isVideo}
    <!-- svelte-ignore a11y_media_has_caption -->
    <video class="aspect-video w-full bg-sunken" controls preload="metadata" {src}></video>
  {:else if inline}
    <img class="max-h-[70vh] w-full bg-sunken object-contain" {src} alt={share.name} />
  {/if}
  <div class="flex flex-col gap-5 p-5 sm:p-6">
    <div class="flex flex-wrap items-center gap-4">
      {#if !inline}
        <span class="grid size-14 flex-none place-items-center rounded-card bg-sunken text-ink-2">
          <FileText class="size-7" aria-hidden="true" />
        </span>
      {/if}
      <div class="min-w-0 flex-1 basis-64">
        <h1 id="file-h" class="text-h1 break-words">{share.name}</h1>
        <p class="mt-1 text-small tabular-nums text-ink-2">{details}</p>
      </div>
      {#if inline}
        <Button
          variant="primary"
          size="lg"
          class="max-sm:w-full"
          loading={downloading}
          onclick={download}
        >
          <Download class="size-4" aria-hidden="true" />Download
        </Button>
      {/if}
    </div>
    {#if !inline}
      <Banner variant="info">
        Preview isn't available for this file type. Download it to open it on your device.
      </Banner>
      <div class="flex justify-end">
        <Button
          variant="primary"
          size="lg"
          class="max-sm:w-full"
          loading={downloading}
          onclick={download}
        >
          <Download class="size-4" aria-hidden="true" />Download
        </Button>
      </div>
    {/if}
    {#if result}
      <p class="text-small text-ink-2" role="status">{result}</p>
    {/if}
  </div>
</article>
