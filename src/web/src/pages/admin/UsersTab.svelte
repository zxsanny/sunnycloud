<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type ApiError } from '$lib/api';
  import { session } from '$lib/session';
  import { Button, EmptyState, Skeleton } from '$lib/ui';
  import SectionError from '../account/SectionError.svelte';
  import { UserList, type AdminUser } from './UserList.svelte';
  import UserRow from './UserRow.svelte';

  const users = new UserList();
  let refusal = $state<ApiError | null>(null);
  let busyId = $state<string | null>(null);

  const items = $derived(users.items ?? []);
  const admins = $derived(items.filter((user) => user.role === 'admin').length);

  /** One server call; the row changes only when the server answers 204. */
  async function change(
    user: AdminUser,
    body: { role?: 'user' | 'admin'; state?: 'active' | 'disabled' },
  ) {
    busyId = user.id;
    refusal = null;
    const result = await api.patchAdminUser(user.id, body);
    busyId = null;
    if (result.ok) users.patch(user.id, body);
    else refusal = result.error;
  }

  onMount(() => void users.load());
</script>

<div id="users-panel" role="tabpanel" aria-labelledby="users-tab" class="flex flex-col gap-4">
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
      aria-label="Loading users"
    >
      {#each [1, 2, 3] as row (row)}
        <Skeleton class="h-10 w-full" />
      {/each}
    </div>
  {:else}
    {#if refusal}
      <SectionError what="The server refused this change." error={refusal} />
    {/if}
    {#if items.length === 0}
      <div class="rounded-card border border-line bg-raised">
        <EmptyState title="No users">The server listed no accounts.</EmptyState>
      </div>
    {:else}
      <p class="text-small text-ink-2 tabular-nums">
        {items.length}
        {items.length === 1 ? 'user' : 'users'} · {admins}
        {admins === 1 ? 'admin' : 'admins'}
      </p>
      <div class="overflow-x-auto rounded-card border border-line bg-raised">
        <table class="w-full text-left text-row">
          <thead class="text-small text-ink-2">
            <tr>
              <th scope="col" class="px-4 py-2 font-[650]">User</th>
              <th scope="col" class="px-4 py-2 font-[650]">Role</th>
              <th scope="col" class="px-4 py-2 font-[650]">Status</th>
              <th scope="col" class="hidden px-4 py-2 font-[650] lg:table-cell">Storage</th>
              <th scope="col" class="hidden px-4 py-2 font-[650] lg:table-cell">Last sign-in</th>
              <th scope="col" class="px-4 py-2"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {#each items as user (user.id)}
              <UserRow
                {user}
                isSelf={user.id === session.account?.id}
                busy={busyId !== null}
                onchange={change}
              />
            {/each}
          </tbody>
        </table>
      </div>
      {#if users.moreError}
        <SectionError what="More users could not be loaded." error={users.moreError} />
      {/if}
      {#if users.hasMore}
        <div>
          <Button loading={users.loadingMore} onclick={() => users.more()}>Show more users</Button>
        </div>
      {/if}
    {/if}
  {/if}
</div>
