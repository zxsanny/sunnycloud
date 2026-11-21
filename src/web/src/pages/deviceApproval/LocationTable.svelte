<script lang="ts">
  import Globe from '@lucide/svelte/icons/globe';
  import User from '@lucide/svelte/icons/user';
  import type { DeviceGrantApproval } from '$lib/api';
  import { placeText } from './view';

  let { approval }: { approval: DeviceGrantApproval } = $props();
</script>

<dl class="divide-y divide-line overflow-hidden rounded-card border border-line bg-raised">
  <div
    class="grid gap-1 px-4 py-3 sm:grid-cols-[10rem_minmax(0,1fr)] sm:items-center sm:gap-3 {approval.mismatch
      ? 'bg-warning-bg text-warning-on'
      : ''}"
    data-testid="requester-place"
  >
    <dt class="flex items-center gap-2.5 text-small font-[650]">
      <Globe class="size-5 flex-none" aria-hidden="true" />Requested from
    </dt>
    <dd class="min-w-0 font-[550]">
      <span class="block">{placeText(approval.requester_location)}</span>
      <span class="block text-small tabular-nums">{approval.requester_ip}</span>
    </dd>
  </div>
  <div
    class="grid gap-1 px-4 py-3 sm:grid-cols-[10rem_minmax(0,1fr)] sm:items-center sm:gap-3"
    data-testid="approver-place"
  >
    <dt class="flex items-center gap-2.5 text-small font-[650] text-ink-2">
      <User class="size-5 flex-none" aria-hidden="true" />You
    </dt>
    <dd class="min-w-0 font-[550]">
      <span class="block">{placeText(approval.approver_location)}</span>
      <span class="block text-small tabular-nums text-ink-2">{approval.approver_ip}</span>
    </dd>
  </div>
</dl>
