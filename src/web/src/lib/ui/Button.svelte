<script lang="ts">
  import Loader from '@lucide/svelte/icons/loader-circle';
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';

  type Variant = 'primary' | 'secondary' | 'ghost' | 'danger' | 'danger-ghost';
  type Size = 'sm' | 'md' | 'lg';

  interface Props extends Omit<HTMLButtonAttributes, 'children'> {
    variant?: Variant;
    size?: Size;
    block?: boolean;
    /** Shows a spinner and blocks clicks; for waits under 3 seconds. */
    loading?: boolean;
    /** Shown as the tooltip while the button is disabled, so a disabled button always says why. */
    disabledReason?: string;
    children: Snippet;
  }

  let {
    variant = 'secondary',
    size = 'md',
    block = false,
    loading = false,
    disabledReason,
    disabled = false,
    type = 'button',
    class: extra = '',
    children,
    ...rest
  }: Props = $props();

  const variants: Record<Variant, string> = {
    primary:
      'border-transparent bg-action text-action-on hover:bg-action-hover active:bg-action-active',
    secondary: 'bg-raised text-ink border border-line-strong hover:bg-sunken active:bg-sunken',
    ghost: 'border-transparent bg-transparent text-ink hover:bg-hover',
    danger: 'border-transparent bg-danger text-action-on hover:bg-danger-on',
    'danger-ghost': 'border-transparent bg-transparent text-danger hover:bg-danger-bg',
  };
  const sizes: Record<Size, string> = {
    sm: 'h-8 px-3 text-small gap-1.5',
    md: 'h-10 px-4 text-body gap-2',
    lg: 'h-12 px-6 text-body gap-2',
  };
  const inert = $derived(disabled || loading);
</script>

<button
  {...rest}
  {type}
  disabled={inert}
  aria-busy={loading || undefined}
  title={disabled ? disabledReason : rest.title}
  class="inline-flex items-center justify-center rounded-control border font-[650] whitespace-nowrap transition-colors duration-150 {sizes[
    size
  ]} {block ? 'w-full' : ''} {variants[
    variant
  ]} disabled:cursor-not-allowed disabled:border-transparent disabled:bg-sunken disabled:text-ink-3 {extra}"
>
  {#if loading}
    <Loader class="size-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
  {/if}
  {@render children()}
</button>
