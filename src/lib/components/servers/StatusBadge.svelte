<script lang="ts">
  import type { ServerStatus } from '$lib/types';
  import { t } from '$lib/i18n.svelte';
  import { STATUS_LABEL, statusError, statusKind } from './utils';
  import { cn } from '$lib/utils';
  import Play from '@lucide/svelte/icons/play';
  import LoaderCircle from '@lucide/svelte/icons/loader-circle';
  import Square from '@lucide/svelte/icons/square';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';

  let { status, class: className }: { status: ServerStatus | undefined; class?: string } = $props();

  let kind = $derived(statusKind(status));
  let error = $derived(statusError(status));

  const styles = {
    running: 'bg-success/10 text-success border-success/30',
    starting: 'bg-warning/10 text-warning border-warning/30',
    stopping: 'bg-warning/10 text-warning border-warning/30',
    stopped: 'bg-muted/50 text-muted-foreground border-border',
    error: 'bg-destructive/10 text-destructive border-destructive/30',
  };

  // Each state also has its own shape (play / spinner / stop / warning), so it reads without colour.
  const icons = {
    running: Play,
    starting: LoaderCircle,
    stopping: LoaderCircle,
    stopped: Square,
    error: TriangleAlert,
  };
  let Icon = $derived(icons[kind]);
</script>

<span
  class={cn(
    'inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-xs font-medium border whitespace-nowrap',
    styles[kind],
    className
  )}
  title={error ?? undefined}
>
  <Icon
    class={cn('size-3 shrink-0', kind === 'running' && 'fill-current', (kind === 'starting' || kind === 'stopping') && 'animate-spin')}
    aria-hidden="true"
  />
  <span>{t(STATUS_LABEL[kind])}</span>
</span>
