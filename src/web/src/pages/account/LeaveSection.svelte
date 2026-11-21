<script lang="ts">
  import LogOut from '@lucide/svelte/icons/log-out';
  import Trash from '@lucide/svelte/icons/trash';
  import { api, type ApiError } from '$lib/api';
  import { navigate } from '$lib/router';
  import { session } from '$lib/session';
  import { Button, CopyButton, Field } from '$lib/ui';
  import { errorText } from './view';

  interface Props {
    /** The server's confirmation for a deletion is the account's own id (contract 07, AccountDeletionService). */
    accountId: string | null;
    ondeleted: (deadline: string) => void;
  }

  let { accountId, ondeleted }: Props = $props();

  let signingOut = $state(false);
  let signOutFailed = $state(false);
  let deleting = $state(false);
  let confirming = $state(false);
  let typed = $state('');
  let refusal = $state<ApiError | null>(null);

  const matches = $derived(accountId !== null && typed.trim() === accountId);

  async function signOut() {
    signingOut = true;
    signOutFailed = false;
    const done = await session.signOut();
    signingOut = false;
    if (done) navigate('/login');
    else signOutFailed = true;
  }

  async function requestDeletion() {
    deleting = true;
    refusal = null;
    const result = await api.deleteAccountsMe(typed.trim());
    deleting = false;
    if (result.ok) {
      ondeleted(result.value.deadline);
      return;
    }
    refusal = result.error;
  }
</script>

<section class="flex flex-col gap-5 xl:pt-1" aria-labelledby="leave-h">
  <h2 id="leave-h" class="sr-only">Sign out and delete</h2>
  <div class="flex flex-col gap-2">
    <h3 class="text-h3">Sign out</h3>
    <p class="text-small text-ink-2">Ends the session in this browser. Your apps stay signed in.</p>
    <Button class="self-start" loading={signingOut} onclick={signOut}>
      <LogOut class="size-4" aria-hidden="true" />Sign out
    </Button>
    {#if signOutFailed}
      <p class="text-small font-semibold text-danger" role="alert">
        Sign-out did not go through. Try again.
      </p>
    {/if}
  </div>
  <hr class="border-line" />
  <div class="flex flex-col gap-2">
    <h3 class="text-h3">Delete account</h3>
    <p class="text-small text-ink-2">
      Removes your files, links, devices and sessions within 15 minutes. Older server backups can
      still contain them.
    </p>
    {#if !confirming}
      <Button
        variant="danger-ghost"
        class="-ml-4 self-start"
        disabled={accountId === null}
        disabledReason="Your account is still loading."
        onclick={() => (confirming = true)}
      >
        <Trash class="size-4" aria-hidden="true" />Delete account…
      </Button>
    {:else}
      <div
        class="flex flex-col gap-3 rounded-card bg-danger-bg p-4 text-danger-on"
        role="group"
        aria-label="Confirm account deletion"
      >
        <p class="text-row">
          <strong class="font-bold">This cannot be undone.</strong> To confirm, type your account ID:
        </p>
        <div class="flex items-center gap-2">
          <code class="min-w-0 flex-1 truncate font-code text-small" data-testid="account-id"
            >{accountId}</code
          >
          {#if accountId}<CopyButton text={accountId} label="Copy account ID" />{/if}
        </div>
        <Field label="Account ID" bind:value={typed} mono autocomplete="off" spellcheck={false} />
        <div class="flex gap-2">
          <Button
            variant="ghost"
            size="sm"
            disabled={deleting}
            onclick={() => {
              confirming = false;
              typed = '';
              refusal = null;
            }}>Cancel</Button
          >
          <Button
            variant="danger"
            size="sm"
            loading={deleting}
            disabled={!matches}
            disabledReason="Type your account ID to confirm."
            onclick={requestDeletion}>Delete my account</Button
          >
        </div>
        {#if refusal}
          <p class="text-small font-semibold" role="alert" data-testid="deletion-refusal">
            {errorText(refusal)}
            {#if refusal.requestId}(reference <code class="font-code">{refusal.requestId}</code
              >){/if}
          </p>
        {/if}
      </div>
    {/if}
  </div>
</section>
