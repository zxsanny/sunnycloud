<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type ApiError } from '$lib/api';
  import { formatBytes } from '$lib/format';
  import { session } from '$lib/session';
  import { Button, EmptyState, Field, Skeleton } from '$lib/ui';
  import SectionError from '../account/SectionError.svelte';
  import { errorText } from '../account/view';
  import { UserList } from './UserList.svelte';
  import { limitBytes, userChoiceLabel, userName, type QuotaUnit } from './view';

  const users = new UserList();
  let userId = $state('');
  let size = $state('');
  let unit = $state<QuotaUnit>('GB');
  let saving = $state(false);
  /** Shown under the form: an invalid limit (nothing was sent) or the server's refusal. */
  let failure = $state<string | null>(null);
  let failureId = $state<string | null>(null);
  let rowError = $state<ApiError | null>(null);

  const items = $derived(users.items ?? []);
  const choices = $derived(items.filter((user) => user.state !== 'deleting'));
  const withOwn = $derived(items.filter((user) => user.limit !== null && user.limit !== undefined));

  async function save(event: SubmitEvent) {
    event.preventDefault();
    failure = null;
    failureId = null;
    if (userId === '') {
      failure = 'Choose a user.';
      return;
    }
    const parsed = limitBytes(size, unit);
    if (!parsed.ok) {
      failure = parsed.reason;
      return;
    }
    saving = true;
    const result = await api.putAdminUserQuota(userId, parsed.bytes);
    saving = false;
    if (result.ok) {
      users.patch(userId, { limit: parsed.bytes });
      size = '';
    } else {
      failure = errorText(result.error);
      failureId = result.error.requestId;
    }
  }

  async function useDefault(id: string) {
    rowError = null;
    const result = await api.putAdminUserQuota(id, null);
    if (result.ok) users.patch(id, { limit: null });
    else rowError = result.error;
  }

  function edit(id: string, limit: number) {
    userId = id;
    failure = null;
    if (limit % 1_000_000_000_000 === 0) {
      unit = 'TB';
      size = String(limit / 1_000_000_000_000);
    } else {
      unit = 'GB';
      size = String(limit / 1_000_000_000);
    }
  }

  onMount(() => void users.load());
</script>

<div
  id="quotas-panel"
  role="tabpanel"
  aria-labelledby="quotas-tab"
  class="grid items-start gap-6 lg:grid-cols-[300px_minmax(0,1fr)]"
>
  <aside class="flex flex-col gap-3" aria-labelledby="default-q-h">
    <h2 id="default-q-h" class="text-h3">Default quota</h2>
    {#if users.defaultQuota !== null}
      <p class="text-h2 tabular-nums" data-testid="default-quota">
        {formatBytes(users.defaultQuota)}
      </p>
    {/if}
    <p class="text-small text-ink-2">
      Applies to every user without their own quota. The operator sets it in the server
      configuration; it cannot be changed here.
    </p>
  </aside>
  <div class="flex flex-col gap-4">
    <h2 class="text-h3">Per-user quotas</h2>
    {#if users.error}
      <SectionError
        what="The users could not be loaded."
        error={users.error}
        onretry={() => users.load()}
      />
    {:else if users.items === null}
      <div
        class="flex flex-col gap-3 rounded-card border border-line bg-raised p-4"
        aria-busy="true"
        aria-label="Loading quotas"
      >
        <Skeleton class="h-10 w-full" />
        <Skeleton class="h-10 w-full" />
      </div>
    {:else}
      <form
        class="grid gap-4 rounded-card border border-line bg-raised p-5 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] md:items-start"
        onsubmit={save}
      >
        <div class="flex flex-col gap-1.5">
          <label class="text-small font-[650]" for="quota-user">User</label>
          <select
            id="quota-user"
            class="h-10 rounded-control border border-line-strong bg-raised px-3 text-ink"
            bind:value={userId}
          >
            <option value="">Choose a user</option>
            {#each choices as user (user.id)}
              <option value={user.id}
                >{userChoiceLabel(user, user.id === session.account?.id)}</option
              >
            {/each}
          </select>
        </div>
        <div class="flex items-start gap-2">
          <div class="flex-1">
            <Field
              label="Quota"
              inputmode="decimal"
              bind:value={size}
              error={failure ?? undefined}
            />
          </div>
          <div class="flex flex-col gap-1.5">
            <label class="text-small font-[650]" for="quota-unit">Unit</label>
            <select
              id="quota-unit"
              class="h-10 rounded-control border border-line-strong bg-raised px-3 text-ink"
              bind:value={unit}
            >
              <option>GB</option>
              <option>TB</option>
            </select>
          </div>
        </div>
        <div class="md:pt-[26px]">
          <Button type="submit" variant="primary" loading={saving}>Save quota</Button>
        </div>
        {#if failureId}
          <p class="text-small text-ink-2 md:col-span-3">
            Reference: <code class="font-code">{failureId}</code>
          </p>
        {/if}
      </form>
      {#if rowError}
        <SectionError what="The quota was not changed." error={rowError} />
      {/if}
      {#if withOwn.length === 0}
        <div class="rounded-card border border-line bg-raised">
          <EmptyState title="Everyone uses the default quota">
            A quota set for one person shows here.
          </EmptyState>
        </div>
      {:else}
        <div class="overflow-x-auto rounded-card border border-line bg-raised">
          <table class="w-full text-left text-row" aria-label="Per-user quotas">
            <thead class="text-small text-ink-2">
              <tr>
                <th scope="col" class="px-4 py-2 font-[650]">User</th>
                <th scope="col" class="px-4 py-2 font-[650]">Quota</th>
                <th scope="col" class="hidden px-4 py-2 font-[650] sm:table-cell">Used</th>
                <th scope="col" class="px-4 py-2"><span class="sr-only">Actions</span></th>
              </tr>
            </thead>
            <tbody>
              {#each withOwn as user (user.id)}
                <tr class="border-t border-line">
                  <td class="px-4 py-3">
                    {#if userName(user) !== user.id}
                      <span class="block text-row">{userName(user)}</span>
                    {/if}
                    <span class="block font-code text-small break-all text-ink-2">{user.id}</span>
                    {#if user.id === session.account?.id}
                      <span class="text-small text-ink-2">(you)</span>
                    {/if}
                  </td>
                  <td class="px-4 py-3 font-semibold tabular-nums"
                    >{formatBytes(user.limit ?? 0)}</td
                  >
                  <td class="hidden px-4 py-3 text-small tabular-nums sm:table-cell"
                    >{formatBytes(user.used)}</td
                  >
                  <td class="px-4 py-3 text-right">
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label="Edit quota of {userName(user)}"
                      onclick={() => edit(user.id, user.limit ?? 0)}>Edit</Button
                    >
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label="Use default quota for {userName(user)}"
                      onclick={() => useDefault(user.id)}>Use default</Button
                    >
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
      <p class="text-small text-ink-2">Everyone else uses the default quota.</p>
      {#if users.hasMore}
        <div>
          <Button loading={users.loadingMore} onclick={() => users.more()}>Show more users</Button>
        </div>
      {/if}
    {/if}
  </div>
</div>
