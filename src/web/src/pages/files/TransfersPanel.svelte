<script lang="ts">
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import Pause from '@lucide/svelte/icons/pause';
  import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
  import Upload from '@lucide/svelte/icons/upload';
  import X from '@lucide/svelte/icons/x';
  import { formatBytes } from '$lib/format';
  import { transfer, type JobProgress } from '$lib/transfer';
  import { Button } from '$lib/ui';

  /** Opens the file picker of the page for the stopped upload with this id. */
  let { onselect }: { onselect: (id: string) => void } = $props();

  const active = $derived(
    transfer.jobs.filter(
      (j) => j.state === 'queued' || j.state === 'running' || j.state === 'committing',
    ).length,
  );
  const percent = (job: JobProgress) =>
    job.size === 0
      ? job.state === 'done'
        ? 100
        : 0
      : Math.min(100, Math.round((job.bytesDone / job.size) * 100));
  const cancellable = (job: JobProgress) =>
    job.state === 'queued' || job.state === 'running' || job.state === 'stopped';
  const finished = (job: JobProgress) =>
    job.state === 'done' || job.state === 'failed' || job.state === 'cancelled';
</script>

{#if transfer.jobs.length > 0}
  <aside
    class="fixed inset-x-0 bottom-0 z-20 rounded-t-card border border-line bg-raised shadow-popup lg:inset-x-auto lg:right-6 lg:bottom-6 lg:w-[420px] lg:rounded-card"
    aria-labelledby="transfers-h"
  >
    <header class="flex h-12 items-center gap-2 border-b border-line px-4">
      <h2 id="transfers-h" class="flex-1 text-h3">
        Transfers <span class="text-small text-ink-2">· {active} active</span>
      </h2>
      {#if transfer.jobs.some(finished)}
        <Button
          variant="ghost"
          size="sm"
          onclick={() => transfer.jobs.filter(finished).forEach((j) => transfer.dismiss(j.id))}
        >
          Clear finished
        </Button>
      {/if}
    </header>
    <ul class="flex max-h-80 flex-col divide-y divide-line overflow-auto">
      {#each transfer.jobs as job (job.id)}
        <li class="flex flex-col gap-2 px-4 py-3">
          <div class="flex items-center gap-2">
            {#if job.state === 'failed'}
              <CircleAlert class="size-4 flex-none text-danger" aria-hidden="true" />
            {:else if job.state === 'done'}
              <CircleCheck class="size-4 flex-none text-success" aria-hidden="true" />
            {:else if job.state === 'stopped'}
              <Pause class="size-4 flex-none text-warning" aria-hidden="true" />
            {:else}
              <Upload class="size-4 flex-none text-ink-2" aria-hidden="true" />
            {/if}
            <span class="min-w-0 flex-1 truncate text-row" title={job.name}>{job.name}</span>
            {#if job.state === 'stopped'}
              <Button
                variant="secondary"
                size="sm"
                aria-label="Select file for {job.name}"
                onclick={() => onselect(job.id)}
              >
                Select file
              </Button>
            {/if}
            {#if cancellable(job)}
              <Button
                variant="ghost"
                size="sm"
                aria-label="Cancel {job.name}"
                onclick={() => transfer.cancelUpload(job.id)}
              >
                <X class="size-4" aria-hidden="true" />
              </Button>
            {/if}
            {#if job.state === 'failed'}
              <Button
                variant="ghost"
                size="sm"
                aria-label="Retry {job.name}"
                onclick={() => transfer.retryUpload(job.id)}
              >
                <RotateCcw class="size-4" aria-hidden="true" />Retry
              </Button>
            {/if}
            {#if finished(job)}
              <Button
                variant="ghost"
                size="sm"
                aria-label="Dismiss {job.name}"
                onclick={() => transfer.dismiss(job.id)}
              >
                <X class="size-4" aria-hidden="true" />
              </Button>
            {/if}
          </div>
          {#if job.state === 'running' || job.state === 'committing'}
            <div
              class="h-1.5 overflow-hidden rounded-full bg-sunken"
              role="progressbar"
              aria-valuenow={percent(job)}
              aria-valuemin="0"
              aria-valuemax="100"
              aria-label="{job.name} uploaded"
            >
              <div class="h-full rounded-full bg-action" style:width="{percent(job)}%"></div>
            </div>
            <p class="text-small text-ink-2">
              {#if job.state === 'committing'}
                Waiting to be saved
              {:else if job.phase === 'preparing'}
                Preparing · {formatBytes(job.bytesDone)} of {formatBytes(job.size)} read
              {:else if job.lanes > 0}
                {formatBytes(job.bytesDone)} of {formatBytes(job.size)} · {job.lanes}
                {job.lanes === 1 ? 'lane' : 'lanes'}
              {:else}
                {formatBytes(job.bytesDone)} of {formatBytes(job.size)}
              {/if}
            </p>
            {#if job.note}
              <p class="text-small text-ink-2">{job.note}</p>
            {/if}
          {:else if job.state === 'stopped'}
            <p class="text-small text-ink-2">{job.errorText}</p>
          {:else if job.state === 'queued'}
            <p class="text-small text-ink-2">Waiting</p>
          {:else if job.state === 'failed'}
            <p class="text-small font-semibold text-danger">Not uploaded: {job.errorText}</p>
          {:else if job.state === 'cancelled'}
            <p class="text-small text-ink-2">Cancelled</p>
          {/if}
        </li>
      {/each}
    </ul>
  </aside>
{/if}
