<script lang="ts">
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import Info from '@lucide/svelte/icons/info';
  import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
  import type { Component, Snippet } from 'svelte';

  type Variant = 'info' | 'success' | 'warning' | 'danger';

  interface Props {
    variant?: Variant;
    /** Replaces the icon of the variant. */
    icon?: Component<{ class?: string; 'aria-hidden'?: boolean | 'true' | 'false' }>;
    children: Snippet;
    /** A button or link on the right. */
    action?: Snippet;
    class?: string;
  }

  let { variant = 'info', icon, children, action, class: extra = '' }: Props = $props();

  const looks: Record<Variant, string> = {
    info: 'bg-action-subtle text-action-on-subtle',
    success: 'bg-success-bg text-success',
    warning: 'bg-warning-bg text-warning-on',
    danger: 'bg-danger-bg text-danger-on',
  };
  const icons = { info: Info, success: CircleCheck, warning: AlertTriangle, danger: CircleAlert };
  const Icon = $derived(icon ?? icons[variant]);
</script>

<div
  role={variant === 'warning' || variant === 'danger' ? 'alert' : 'status'}
  class="flex items-start gap-3 rounded-card px-4 py-3 text-row {looks[variant]} {extra}"
>
  <Icon class="mt-px size-5 flex-none" aria-hidden="true" />
  <div class="min-w-0 flex-1">{@render children()}</div>
  {#if action}
    <div class="flex-none">{@render action()}</div>
  {/if}
</div>
