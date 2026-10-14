<script lang="ts">
  // Settings > Diagnostics: the `harness doctor` report (read-only checks, run on demand).
  import { invoke } from '#lib/ipc.js';
  import { onMount } from 'svelte';
  import { toast } from 'svelte-sonner';
  import { CircleCheck, CircleX, TriangleAlert, LoaderCircle, RefreshCw, Copy } from '@lucide/svelte';
  import type { DoctorCheck, DoctorReport, DoctorStatus } from '#lib/types.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { cn } from '#lib/utils.js';
  import SettingGroup from './SettingGroup.svelte';
  import SectionLoading from '#lib/components/common/SectionLoading.svelte';

  let report = $state<DoctorReport | null>(null);
  let running = $state(false);
  let error = $state<string | null>(null);

  const STATUS: Record<DoctorStatus, { key: TKey; icon: typeof CircleCheck; tone: string }> = {
    ok: { key: 'doctor.status_ok', icon: CircleCheck, tone: 'text-success' },
    warn: { key: 'doctor.status_warn', icon: TriangleAlert, tone: 'text-warning' },
    fail: { key: 'doctor.status_fail', icon: CircleX, tone: 'text-destructive' },
  };

  async function run() {
    running = true;
    error = null;
    try {
      report = await invoke<DoctorReport>('run_doctor');
    } catch (e) {
      error = localizeError(e);
    } finally {
      running = false;
    }
  }

  onMount(run);

  const title = (c: DoctorCheck) => t(`doctor.check.${c.id}` as TKey);
  const message = (c: DoctorCheck) => t(c.message_key as TKey, c.params);

  async function copyJson() {
    if (!report) return;
    try {
      await navigator.clipboard.writeText(JSON.stringify(report, null, 2));
      toast.success(t('doctor.copied'));
    } catch (e) {
      toast.error(String(e));
    }
  }
</script>

<div class="flex flex-col gap-6">
  <SettingGroup title={t('settings.tab_diagnostics')}>
    <div class="flex flex-wrap items-center gap-2 px-3 py-2.5">
      <Button size="sm" variant="outline" onclick={run} disabled={running}>
        {#if running}<LoaderCircle class="animate-spin" />{t('doctor.running')}{:else}<RefreshCw />{t('doctor.run')}{/if}
      </Button>
      <Button size="sm" variant="ghost" onclick={copyJson} disabled={!report}>
        <Copy />
        {t('doctor.copy_json')}
      </Button>
      {#if report && !running}
        <span class={cn('text-xs', report.ok ? 'text-success' : 'text-destructive')} role="status">
          {report.ok ? t('doctor.all_ok') : t('doctor.has_fail')}
        </span>
      {/if}
    </div>
    {#if error}
      <p class="mx-3 mb-2.5 rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-2 text-xs text-destructive" role="alert">{error}</p>
    {/if}
    {#if !report && running}
      <SectionLoading shape="rows" rows={4} class="m-3" label={t('sl.settings_label')} phases={[t('sl.settings_2'), t('sl.settings_1')]} />
    {:else if report}
      <ul class="flex flex-col divide-y divide-border border-t border-border">
        {#each report.checks as c (c.id)}
          {@const s = STATUS[c.status]}
          <li class="flex items-start gap-2.5 px-3 py-2 text-xs">
            <s.icon class={cn('mt-0.5 size-3.5 shrink-0', s.tone)} aria-label={t(s.key)} />
            <div class="flex min-w-0 flex-1 flex-col gap-0.5">
              <span class="font-medium">{title(c)}</span>
              <span class="break-words text-muted-foreground">{message(c)}</span>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </SettingGroup>
</div>
