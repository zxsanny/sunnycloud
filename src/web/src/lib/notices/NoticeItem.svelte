<script lang="ts">
  import X from '@lucide/svelte/icons/x';
  import { Banner, Button, CopyButton } from '$lib/ui';
  import { notices, type Notice } from './notices.svelte';

  let { notice }: { notice: Notice } = $props();

  let now = $state(Date.now());
  const secondsLeft = $derived(
    notice.retryAt === undefined ? 0 : Math.max(0, Math.ceil((notice.retryAt - now) / 1000)),
  );

  $effect(() => {
    const until = notice.retryAt;
    if (until === undefined) return;
    const timer = setInterval(() => {
      now = Date.now();
      if (now >= until) notices.dismiss(notice.key);
    }, 1000);
    return () => clearInterval(timer);
  });
</script>

<Banner variant={notice.variant}>
  <strong class="font-bold">{notice.title}</strong>
  {#if notice.retryAt !== undefined}
    Try again in <span>{secondsLeft}</span> {secondsLeft === 1 ? 'second' : 'seconds'}.
  {:else}
    {notice.text}
  {/if}
  {#if notice.requestId}
    <span class="font-code text-code">{notice.requestId}</span>
  {/if}
  {#snippet action()}
    <div class="flex items-center gap-1">
      {#if notice.requestId}
        <CopyButton text={notice.requestId} label="Copy reference" />
      {/if}
      {#if notice.action === 'reload'}
        <Button size="sm" onclick={() => globalThis.location.reload()}>Reload</Button>
      {/if}
      {#if notice.dismissible}
        <Button
          variant="ghost"
          size="sm"
          aria-label="Dismiss"
          onclick={() => notices.dismiss(notice.key)}
        >
          <X class="size-4" aria-hidden="true" />
        </Button>
      {/if}
    </div>
  {/snippet}
</Banner>
