<script lang="ts">
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import { onMount } from 'svelte';
  import { api, type ApiError, type InviteList } from '$lib/api';
  import { formatDate } from '$lib/format';
  import { Button, CopyButton, EmptyState, Field, Skeleton } from '$lib/ui';
  import SectionError from '../account/SectionError.svelte';
  import { errorText } from '../account/view';
  import { inviteStatus } from './view';

  type Invite = InviteList['items'][number];

  let invites = $state<Invite[] | null>(null);
  let loadError = $state<ApiError | null>(null);
  let note = $state('');
  let creating = $state(false);
  let createError = $state<ApiError | null>(null);
  /** The plaintext token exists only in the create answer: it lives here, in memory, until the panel is closed. */
  let created = $state<{ token: string; note: string; expires: string | null } | null>(null);
  let revokingId = $state<string | null>(null);
  let revokeError = $state<ApiError | null>(null);

  async function load(): Promise<Invite[] | null> {
    loadError = null;
    const result = await api.getAdminInvites();
    if (result.ok) {
      invites = result.value.items;
      return invites;
    }
    loadError = result.error;
    return null;
  }

  async function create(event: SubmitEvent) {
    event.preventDefault();
    creating = true;
    createError = null;
    created = null;
    const before = new Set((invites ?? []).map((invite) => invite.id));
    const result = await api.postAdminInvite(note.trim());
    if (!result.ok) {
      creating = false;
      createError = result.error;
      return;
    }
    const token = result.value.token;
    const forNote = note.trim();
    note = '';
    // The create answer carries no expiry: it is read from the one invite the list gained.
    const fresh = ((await load()) ?? []).filter((invite) => !before.has(invite.id));
    created = {
      token,
      note: forNote,
      expires: fresh.length === 1 ? (fresh[0]?.expires_at ?? null) : null,
    };
    creating = false;
  }

  async function revoke(invite: Invite) {
    revokingId = invite.id;
    revokeError = null;
    const result = await api.deleteAdminInvite(invite.id);
    revokingId = null;
    if (result.ok) invites = (invites ?? []).filter((other) => other.id !== invite.id);
    else revokeError = result.error;
  }

  const STATUS_TEXT = { pending: 'Pending', used: 'Used', expired: 'Expired' } as const;

  onMount(() => void load());
</script>

<div id="invites-panel" role="tabpanel" aria-labelledby="invites-tab" class="flex flex-col gap-5">
  <form
    class="flex flex-col gap-3 rounded-card border border-line bg-raised p-5 lg:flex-row lg:items-start"
    onsubmit={create}
  >
    <div class="flex-1">
      <Field
        label="Note (optional)"
        placeholder="Who is it for?"
        bind:value={note}
        helper="Only admins see the note. An invite works once and expires on the date the server sets."
        error={createError ? errorText(createError) : undefined}
      />
    </div>
    <div class="lg:pt-[26px]">
      <Button type="submit" variant="primary" loading={creating}>Create invite</Button>
    </div>
  </form>

  {#if created}
    <section
      class="flex flex-col gap-3 rounded-card bg-sun-fill p-4 text-sun-on"
      aria-labelledby="created-h"
    >
      <h2 id="created-h" class="flex items-center gap-2 text-h3">
        <CircleCheck class="size-5" aria-hidden="true" />
        {created.note ? `Invite created for ${created.note}` : 'Invite created'}
      </h2>
      <p
        class="rounded-control border border-line bg-raised p-3 font-code text-small break-all text-ink"
        data-testid="invite-token"
      >
        {created.token}
      </p>
      <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
        <CopyButton text={created.token} label="Copy invite token" />
        <p class="text-small font-semibold">
          This is the only time it is shown. Send it to the person directly.
        </p>
        <Button size="sm" variant="ghost" onclick={() => (created = null)}>Close</Button>
      </div>
      <p class="text-small tabular-nums">
        Single use{created.expires ? ` · expires ${formatDate(created.expires)}` : ''}
      </p>
    </section>
  {/if}

  {#if revokeError}
    <SectionError what="The invite was not revoked." error={revokeError} />
  {/if}

  {#if loadError}
    <SectionError what="The invites could not be loaded." error={loadError} onretry={load} />
  {:else if invites === null}
    <div
      class="flex flex-col gap-3 rounded-card border border-line bg-raised p-4"
      aria-busy="true"
      aria-label="Loading invites"
    >
      {#each [1, 2] as row (row)}
        <Skeleton class="h-10 w-full" />
      {/each}
    </div>
  {:else if invites.length === 0}
    <div class="rounded-card border border-line bg-raised">
      <EmptyState title="No invites yet">
        New people join only with an invite. Create one above; it shows here with its status.
      </EmptyState>
    </div>
  {:else}
    <div class="overflow-x-auto rounded-card border border-line bg-raised">
      <table class="w-full text-left text-row">
        <thead class="text-small text-ink-2">
          <tr>
            <th scope="col" class="px-4 py-2 font-[650]">Invite</th>
            <th scope="col" class="hidden px-4 py-2 font-[650] sm:table-cell">Created</th>
            <th scope="col" class="hidden px-4 py-2 font-[650] sm:table-cell">Expires</th>
            <th scope="col" class="px-4 py-2 font-[650]">Status</th>
            <th scope="col" class="px-4 py-2"><span class="sr-only">Actions</span></th>
          </tr>
        </thead>
        <tbody>
          {#each invites as invite (invite.id)}
            {@const status = inviteStatus(invite)}
            <tr class="border-t border-line">
              <td class="px-4 py-3">
                {#if invite.note}{invite.note}{:else}<span class="text-ink-3">No note</span>{/if}
              </td>
              <td class="hidden px-4 py-3 text-small tabular-nums sm:table-cell"
                >{formatDate(invite.created_at)}</td
              >
              <td class="hidden px-4 py-3 text-small tabular-nums sm:table-cell"
                >{formatDate(invite.expires_at)}</td
              >
              <td class="px-4 py-3 text-small font-semibold">{STATUS_TEXT[status]}</td>
              <td class="px-4 py-3 text-right">
                {#if status === 'pending'}
                  <Button
                    size="sm"
                    variant="danger-ghost"
                    aria-label="Revoke invite {invite.note || 'without a note'}"
                    loading={revokingId === invite.id}
                    onclick={() => revoke(invite)}>Revoke</Button
                  >
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>
