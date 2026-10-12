<script lang="ts">
  import { localizeError } from '$lib/network';
  import { errorCode } from '$lib/errors';
  import { t, locale, type TKey } from '$lib/i18n.svelte';
  import { concurrentServers, runToMarkdown, type BenchmarkRun, type RunStatus } from '$lib/benchmark-types';
  import BenchResultsTable from './BenchResultsTable.svelte';
  import { Card, CardHeader, CardTitle, CardContent } from '$lib/components/ui/card';
  import { Button } from '$lib/components/ui/button';
  import { Badge } from '$lib/components/ui/badge';
  import { X, Copy, Check, AlertCircle } from '@lucide/svelte';

  interface Props {
    run: BenchmarkRun;
    onClose: () => void;
  }

  let { run, onClose }: Props = $props();

  let copyState = $state<{ id: string; ok: boolean } | null>(null);

  const statusKey: Record<RunStatus, TKey> = {
    completed: 'bench.status_completed',
    cancelled: 'bench.status_cancelled',
    error: 'bench.status_error',
  };

  const statusBadgeVariant: Record<RunStatus, 'default' | 'secondary' | 'destructive' | 'outline'> = {
    completed: 'secondary',
    cancelled: 'outline',
    error: 'destructive',
  };

  function formatDate(secs: number): string {
    return new Date(secs * 1000).toLocaleString(locale());
  }

  async function copyMarkdown(r: BenchmarkRun) {
    try {
      await navigator.clipboard.writeText(runToMarkdown(r));
      copyState = { id: r.id, ok: true };
    } catch {
      copyState = { id: r.id, ok: false };
    }
    const id = r.id;
    setTimeout(() => {
      if (copyState?.id === id) copyState = null;
    }, 2000);
  }
</script>

<Card class="border-border">
  <CardHeader class="pb-3 border-b border-border">
    <div class="flex items-center justify-between gap-4 flex-wrap">
      <div>
        <CardTitle class="text-base font-semibold">{t('bench.results_title')}</CardTitle>
        <div class="text-xs text-muted-foreground mt-0.5 flex items-center gap-2 flex-wrap">
          <span class="font-medium text-foreground">{run.model_name}</span>
          <span>·</span>
          <span>{formatDate(run.started_at)}</span>
          <span>·</span>
          <span>{t('bench.duration', { seconds: Math.max(0, run.finished_at - run.started_at) })}</span>
        </div>
      </div>
      <div class="flex items-center gap-2">
        <Badge variant={statusBadgeVariant[run.status]} class="text-xs">
          {t(errorCode(run.error) === 'benchmark.timeout' ? 'bench.status_timeout' : statusKey[run.status])}
        </Badge>
        {#if run.results.length > 0}
          <Button variant="outline" size="sm" onclick={() => copyMarkdown(run)}>
            {#if copyState?.id === run.id && copyState.ok}
              <Check size={13} class="mr-1 text-success" />
              {t('bench.copied')}
            {:else if copyState?.id === run.id && !copyState.ok}
              <AlertCircle size={13} class="mr-1 text-destructive" />
              {t('bench.copy_failed')}
            {:else}
              <Copy size={13} class="mr-1" />
              {t('bench.copy_md')}
            {/if}
          </Button>
        {/if}
        <Button variant="ghost" size="sm" class="h-8 w-8 p-0" aria-label={t('bench.close')} onclick={onClose}>
          <X size={14} />
        </Button>
      </div>
    </div>
  </CardHeader>
  <CardContent class="pt-4 flex flex-col gap-3">
    {#if run.status === 'cancelled'}
      <div class="px-3 py-2 rounded-lg bg-warning/10 border border-warning/30 text-xs text-warning">
        {t('bench.run_cancelled_note')}
      </div>
    {/if}
    {#if concurrentServers(run) > 0}
      <div class="px-3 py-2 rounded-lg bg-warning/10 border border-warning/30 text-xs text-warning">
        {t('bench.run_busy_note', { n: concurrentServers(run) })}
      </div>
    {/if}
    {#if run.error}
      <div class="p-3 rounded-lg bg-destructive/10 border border-destructive/20 text-xs text-destructive flex flex-col gap-1">
        <strong class="font-semibold">{t('bench.run_error')}</strong>
        <pre class="font-mono text-xs whitespace-pre-wrap">{localizeError(run.error)}</pre>
      </div>
    {/if}
    {#if run.results.length > 0}
      <BenchResultsTable results={run.results} />
    {:else if !run.error}
      <div class="py-8 text-center text-xs text-muted-foreground">{t('bench.no_results')}</div>
    {/if}
  </CardContent>
</Card>
