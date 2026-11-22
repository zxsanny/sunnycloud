<script lang="ts">
  import Shield from '@lucide/svelte/icons/shield';
  import { formatBytes, formatDate } from '$lib/format';
  import { Button } from '$lib/ui';
  import type { AdminUser } from './UserList.svelte';
  import { userName } from './view';

  interface Props {
    user: AdminUser;
    isSelf: boolean;
    busy: boolean;
    onchange: (
      user: AdminUser,
      change: { role?: 'user' | 'admin'; state?: 'active' | 'disabled' },
    ) => void;
  }

  let { user, isSelf, busy, onchange }: Props = $props();

  const deleting = $derived(user.state === 'deleting');
  const percent = $derived(
    user.limit ? Math.min(100, Math.round((user.used / user.limit) * 100)) : null,
  );
</script>

<tr class="border-t border-line align-top">
  <td class="px-4 py-3">
    {#if userName(user) !== user.id}
      <span class="block text-row" data-testid="user-name">{userName(user)}</span>
      <span class="block font-code text-small break-all text-ink-2" data-testid="user-id"
        >{user.id}</span
      >
    {:else}
      <span class="block font-code text-small break-all" data-testid="user-id">{user.id}</span>
    {/if}
    {#if isSelf}<span class="text-small text-ink-2">(you)</span>{/if}
    {#if deleting}
      <span class="block text-small text-ink-2">Removing data…</span>
    {/if}
  </td>
  <td class="px-4 py-3">
    {#if user.role === 'admin'}
      <span
        class="inline-flex items-center gap-1 rounded-full bg-sun-fill px-2 py-0.5 text-small font-semibold text-sun-on"
        ><Shield class="size-3.5" aria-hidden="true" />Admin</span
      >
    {:else}
      <span class="rounded-full bg-sunken px-2 py-0.5 text-small font-semibold">User</span>
    {/if}
  </td>
  <td class="px-4 py-3 text-small font-semibold">
    {user.state === 'active' ? 'Active' : user.state === 'disabled' ? 'Deactivated' : 'Deleting'}
  </td>
  <td class="hidden px-4 py-3 text-small tabular-nums lg:table-cell">
    {#if deleting}
      —
    {:else}
      {formatBytes(user.used)}
      {#if user.limit}
        / {formatBytes(user.limit)}
        <div
          class="mt-1 h-1.5 w-24 overflow-hidden rounded-full bg-sunken"
          role="progressbar"
          aria-label="Storage used"
          aria-valuemin="0"
          aria-valuemax="100"
          aria-valuenow={percent}
        >
          <div class="h-full bg-action" style="width: {percent}%"></div>
        </div>
      {:else}
        <span class="text-ink-2">· default limit</span>
      {/if}
    {/if}
  </td>
  <td class="hidden px-4 py-3 text-small tabular-nums lg:table-cell">
    {user.last_login ? formatDate(user.last_login) : 'Never'}
  </td>
  <td class="px-4 py-3">
    {#if !deleting}
      <div class="flex flex-wrap justify-end gap-1">
        <Button
          size="sm"
          variant="ghost"
          disabled={busy}
          onclick={() => onchange(user, { role: user.role === 'admin' ? 'user' : 'admin' })}
          >{user.role === 'admin' ? 'Make user' : 'Make admin'}</Button
        >
        <Button
          size="sm"
          variant="ghost"
          disabled={busy}
          onclick={() => onchange(user, { state: user.state === 'active' ? 'disabled' : 'active' })}
          >{user.state === 'active' ? 'Deactivate' : 'Reactivate'}</Button
        >
      </div>
    {/if}
  </td>
</tr>
