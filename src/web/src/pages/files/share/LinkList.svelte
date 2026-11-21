<script lang="ts">
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import Link from '@lucide/svelte/icons/link';
  import Lock from '@lucide/svelte/icons/lock';
  import { formatDate } from '$lib/format';
  import type { OwnerShares, ShareInfo } from '$lib/shares';
  import { Button } from '$lib/ui';

  let { shares }: { shares: OwnerShares } = $props();

  let confirming = $state<string | null>(null);

  const expired = (item: ShareInfo) => Date.parse(item.expires_at) <= Date.now();
  const active = $derived(shares.items.filter((item) => !expired(item)).length);

  async function revoke(id: string) {
    await shares.revoke(id);
    if (shares.revokeFailure === null) confirming = null;
  }
</script>

{#if shares.items.length === 0}
  <p class="text-small text-ink-2">This file has no links yet.</p>
{:else}
  <p class="text-small tabular-nums text-ink-2">{active} active</p>
  <ul class="divide-y divide-line overflow-hidden rounded-card border border-line">
    {#each shares.items as item (item.id)}
      <li class="flex flex-col">
        <div class="flex items-center gap-3 px-4 py-3">
          <span
            class="grid size-9 flex-none place-items-center rounded-control bg-sunken text-ink-2"
          >
            <Link class="size-4" aria-hidden="true" />
          </span>
          <div class="min-w-0 flex-1">
            <p class="flex flex-wrap items-center gap-2 text-row">
              {expired(item) ? 'Expired' : 'Expires'}
              {formatDate(item.expires_at)}
              {#if shares.created?.share.id === item.id}
                <span class="rounded-full bg-sun-fill px-2 text-label text-sun-on">New</span>
              {/if}
            </p>
            <p class="flex flex-wrap items-center gap-x-3 text-small text-ink-2">
              <span>Created {formatDate(item.created_at)}</span>
              {#if item.has_password}
                <span class="inline-flex items-center gap-1">
                  <Lock class="size-4" aria-hidden="true" />Password
                </span>
              {:else}
                <span>No password</span>
              {/if}
            </p>
          </div>
          {#if confirming !== item.id}
            <Button
              variant="danger-ghost"
              size="sm"
              aria-label="Revoke link created {formatDate(item.created_at)}"
              onclick={() => (confirming = item.id)}
            >
              Revoke
            </Button>
          {/if}
        </div>
        {#if confirming === item.id}
          <div
            class="flex flex-wrap items-center gap-3 bg-danger-bg px-4 py-3"
            role="group"
            aria-label="Confirm revoke"
          >
            <p
              class="flex flex-1 basis-52 items-start gap-2 text-small font-semibold text-danger-on"
            >
              <CircleAlert class="mt-0.5 size-4 flex-none" aria-hidden="true" />People who have this
              link lose access.
            </p>
            <div class="ml-auto flex gap-2">
              <Button
                size="sm"
                disabled={shares.revoking !== null}
                onclick={() => (confirming = null)}
              >
                Keep
              </Button>
              <Button
                variant="danger"
                size="sm"
                loading={shares.revoking === item.id}
                onclick={() => void revoke(item.id)}
              >
                Revoke link
              </Button>
            </div>
            {#if shares.revokeFailure?.id === item.id}
              <p class="basis-full text-small font-semibold text-danger-on" role="alert">
                {shares.revokeFailure.text}
              </p>
            {/if}
          </div>
        {/if}
      </li>
    {/each}
  </ul>
{/if}
