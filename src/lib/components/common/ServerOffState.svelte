<script lang="ts">
  // One empty state for "the selected server is not running": replaces grids of 0.0 / – / "not
  // available" and empty chart axes with what is going on and what to do next.
  import type { ServerStatus } from '$lib/types';
  import { t } from '$lib/i18n.svelte';
  import { app } from '$lib/stores/app.svelte';
  import { serverErrorText } from '$lib/failure';
  import { cn } from '$lib/utils';
  import { Button } from '$lib/components/ui/button';
  import { LoaderCircle, Play, PowerOff, TriangleAlert } from '@lucide/svelte';

  interface Props {
    status: ServerStatus;
    /** What the screen shows once the server runs ("speed, memory and requests"). */
    description: string;
    /** After a failed start or crash: what to do now (default: fix it and start again). */
    failedDescription?: string;
    /** The error is already shown next to the start control: only the title and the hint here. */
    hideError?: boolean;
    /** Starts the server right here (omitted = the start control is elsewhere on screen). */
    onStart?: () => void;
    startLabel?: string;
    /** A second way forward, e.g. "Open Dashboard". */
    secondary?: { label: string; onclick: () => void };
    busy?: boolean;
    class?: string;
  }

  let { status, description, failedDescription, hideError = false, onStart, startLabel, secondary, busy = false, class: className }: Props = $props();

  let starting = $derived(status === 'Starting');
  let failed = $derived(typeof status === 'object');
  let error = $derived(failed && !hideError ? serverErrorText(app.selectedServer) : null);
</script>

<div class={cn('flex flex-col items-center justify-center gap-3 px-6 py-10 text-center', className)} role="status">
  <span class="flex items-center justify-center text-muted-foreground" aria-hidden="true">
    {#if starting}
      <LoaderCircle class="size-5 animate-spin" />
    {:else if failed}
      <TriangleAlert class="size-5 text-destructive" />
    {:else}
      <PowerOff class="size-5" />
    {/if}
  </span>
  <div class="flex max-w-md flex-col gap-1">
    <p class="text-base font-semibold text-foreground">
      {starting ? t('ui.off_starting_title') : failed ? t('ui.off_failed_title') : t('ui.off_title', { name: app.serverLabel(app.selectedServerId) })}
    </p>
    {#if error}
      <p class="text-sm break-words text-destructive">{error}</p>
    {/if}
    <p class="text-sm text-muted-foreground">{starting ? t('ui.off_starting_desc') : failed ? (failedDescription ?? t('ui.off_failed_desc')) : description}</p>
  </div>
  {#if !starting && (onStart || secondary)}
    <div class="flex flex-wrap items-center justify-center gap-2">
      {#if onStart}
        <Button size="sm" onclick={onStart} disabled={busy}>
          <Play class="size-3.5" />
          {startLabel ?? t('ui.off_start')}
        </Button>
      {/if}
      {#if secondary}
        <Button size="sm" variant={onStart ? 'ghost' : 'outline'} onclick={secondary.onclick}>{secondary.label}</Button>
      {/if}
    </div>
  {/if}
</div>
