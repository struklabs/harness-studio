<script lang="ts">
  import { onDestroy } from 'svelte';
  import { toast } from 'svelte-sonner';
  import { Check, Copy } from '@lucide/svelte';
  import { Button, type ButtonSize, type ButtonVariant } from '$lib/components/ui/button';
  import * as Tooltip from '$lib/components/ui/tooltip';
  import { t } from '$lib/i18n.svelte';

  interface Props {
    text: string;
    /** Visible label; when omitted the button is icon-only. */
    label?: string;
    copiedLabel?: string;
    /** Accessible name / tooltip for the icon-only form. */
    ariaLabel?: string;
    variant?: ButtonVariant;
    size?: ButtonSize;
    class?: string;
    disabled?: boolean;
  }

  let {
    text,
    label,
    copiedLabel,
    ariaLabel,
    variant = 'ghost',
    size = 'icon-sm',
    class: className,
    disabled = false,
  }: Props = $props();

  let copied = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  onDestroy(() => clearTimeout(timer));

  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      copied = true;
      clearTimeout(timer);
      timer = setTimeout(() => (copied = false), 1800);
    } catch {
      copied = false;
      toast.error(t('conn.copy_failed'));
    }
  }

  let name = $derived(ariaLabel ?? label ?? t('conn.copy'));
</script>

{#if label}
  <Button {variant} {size} class={className} {disabled} onclick={copy}>
    {#if copied}<Check class="text-success" />{:else}<Copy />{/if}
    {copied ? (copiedLabel ?? t('conn.copied')) : label}
  </Button>
{:else}
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <Button {...props} {variant} {size} class={className} {disabled} aria-label={copied ? t('conn.copied') : name} onclick={copy}>
          {#if copied}<Check class="text-success" />{:else}<Copy />{/if}
        </Button>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content>{copied ? t('conn.copied') : name}</Tooltip.Content>
  </Tooltip.Root>
{/if}
