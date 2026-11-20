<script lang="ts">
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import type { Snippet } from 'svelte';
  import type { HTMLInputAttributes } from 'svelte/elements';

  interface Props extends Omit<HTMLInputAttributes, 'children' | 'value'> {
    label: string;
    value?: string;
    helper?: string;
    /** Shown under the field with an alert icon, and marks the input invalid. */
    error?: string;
    mono?: boolean;
    /** Extra content under the helper text. */
    children?: Snippet;
  }

  let {
    label,
    value = $bindable(''),
    helper,
    error,
    mono = false,
    children,
    class: extra = '',
    ...rest
  }: Props = $props();

  const id = $props.id();
  const describedBy = $derived(
    [error ? `${id}-error` : null, helper ? `${id}-help` : null].filter(Boolean).join(' ') ||
      undefined,
  );
</script>

<div class="flex flex-col gap-1.5">
  <label class="text-small font-[650] text-ink" for={id}>{label}</label>
  <input
    {...rest}
    {id}
    bind:value
    aria-invalid={error ? 'true' : undefined}
    aria-describedby={describedBy}
    class="h-10 w-full rounded-control border bg-raised px-3 text-ink placeholder:text-ink-3 hover:border-ink-2 focus-visible:border-action disabled:cursor-not-allowed disabled:border-line disabled:bg-sunken disabled:text-ink-3 {error
      ? 'border-danger'
      : 'border-line-strong'} {mono ? 'font-code tracking-[0.04em]' : ''} {extra}"
  />
  {#if error}
    <p id="{id}-error" class="flex items-start gap-1.5 text-[0.8125rem] font-semibold text-danger">
      <CircleAlert class="mt-px size-4 flex-none" aria-hidden="true" />{error}
    </p>
  {/if}
  {#if helper}
    <p id="{id}-help" class="text-[0.8125rem] text-ink-2">{helper}</p>
  {/if}
  {@render children?.()}
</div>
