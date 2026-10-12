<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { toast } from 'svelte-sonner';
  import { ShieldCheck, Copy, Check } from '@lucide/svelte';
  import type { DiagnosticPayload } from '#lib/types.js';
  import { t, locale, type TKey } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import * as Collapsible from '#lib/components/ui/collapsible/index.js';
  import type { SettingsDraft, DraftKey } from './form.svelte';
  import SettingGroup from './SettingGroup.svelte';
  import SettingRow from './SettingRow.svelte';
  import SwitchRow from './SwitchRow.svelte';

  interface Props {
    page?: string;
    form: SettingsDraft;
  }

  let { page = 'privacy', form }: Props = $props();

  const toggles: { key: DraftKey; label: TKey; desc: TKey }[] = [
    { key: 'crash_reports', label: 'settings.crash_reports_label', desc: 'settings.crash_reports_desc' },
    { key: 'app_errors', label: 'settings.app_errors_label', desc: 'settings.app_errors_desc' },
    { key: 'llama_errors', label: 'settings.llama_errors_label', desc: 'settings.llama_errors_desc' },
    { key: 'hardware_info', label: 'settings.hardware_info_label', desc: 'settings.hardware_info_desc' },
    { key: 'performance_metrics', label: 'settings.perf_metrics_label', desc: 'settings.perf_metrics_desc' },
    { key: 'anonymous_analytics', label: 'settings.usage_analytics_label', desc: 'settings.usage_analytics_desc' },
  ];

  let reports = $state<DiagnosticPayload[]>([]);
  let showReports = $state(false);
  let copied = $state(false);
  let clearing = $state(false);
  let confirmClear = $state(false);
  let copiedTimer: ReturnType<typeof setTimeout> | null = null;
  let destroyed = false;

  async function loadReports() {
    try {
      const res = await invoke<DiagnosticPayload[]>('list_diagnostic_reports');
      if (!destroyed) reports = Array.isArray(res) ? res : [];
    } catch {
      if (!destroyed) reports = [];
    }
  }

  async function clearReports() {
    if (clearing) return;
    clearing = true;
    try {
      await invoke('clear_diagnostic_reports');
      confirmClear = false;
      showReports = false;
      toast.success(t('settings.reports_cleared'));
    } catch (e) {
      toast.error(t('settings.reports_clear_failed', { error: String(e) }));
    } finally {
      await loadReports();
      clearing = false;
    }
  }

  async function copyReports() {
    try {
      await navigator.clipboard.writeText(JSON.stringify($state.snapshot(reports), null, 2));
      copied = true;
      if (copiedTimer) clearTimeout(copiedTimer);
      copiedTimer = setTimeout(() => (copied = false), 2000);
    } catch (e) {
      toast.error(t('settings.reports_copy_failed', { error: String(e) }));
    }
  }

  onMount(loadReports);
  onDestroy(() => {
    destroyed = true;
    if (copiedTimer) clearTimeout(copiedTimer);
  });
</script>

<div class="flex flex-col gap-6">
<div hidden={page !== 'privacy'} class="flex flex-col gap-3">
  <SettingGroup title={t('settings.privacy_title')} note={t('settings.privacy_desc')}>
    {#each toggles as tg (tg.key)}
      <SwitchRow
        id="privacy-{tg.key}"
        label={t(tg.label)}
        hint={t(tg.desc)}
        checked={form.get(tg.key) as boolean}
        dirty={form.isDirty(tg.key)}
        onchange={(v) => form.set(tg.key, v as never)}
      />
    {/each}
  </SettingGroup>

  <div class="flex items-start gap-2.5 rounded-lg border border-border bg-card px-4 py-3">
    <ShieldCheck class="mt-0.5 size-4 shrink-0 text-success" />
    <div class="flex min-w-0 flex-col gap-0.5">
      <span class="text-sm font-medium">{t('settings.privacy_guarantee_title')}</span>
      <p class="text-xs text-muted-foreground">
        {t('settings.privacy_guarantee_text')} <strong class="text-foreground">{t('settings.privacy_guarantee_bold')}</strong>.
      </p>
    </div>
  </div>

</div>
<div hidden={page !== 'reports'} class="flex flex-col gap-6">  <SettingGroup title={t('settings.reports_title')} hint={t('settings.reports_desc')}>
    <SettingRow label={t('settings.local_reports_title', { count: reports.length })} note={reports.length === 0 ? t('settings.reports_empty') : undefined}>
      {#snippet extra()}
        {#if showReports && reports.length > 0}
          <div class="flex max-h-72 flex-col gap-1 overflow-auto">
            {#each reports as r, i (i)}
              <Collapsible.Root class="rounded-md border border-border">
                <Collapsible.Trigger class="flex w-full min-w-0 items-center gap-2 px-2.5 py-1.5 text-left text-xs hover:bg-muted/50">
                  <Badge variant="outline" class="shrink-0 font-mono">{r.event_type}</Badge>
                  <span class="shrink-0 text-muted-foreground tabular">{new Date(r.timestamp * 1000).toLocaleString(locale())}</span>
                  <span class="min-w-0 truncate text-muted-foreground">{r.sanitized_message.slice(0, 120)}</span>
                </Collapsible.Trigger>
                <Collapsible.Content>
                  <pre class="selectable max-h-56 overflow-auto border-t border-border bg-muted/30 p-2.5 font-mono text-xs whitespace-pre-wrap">{JSON.stringify(r, null, 2)}</pre>
                </Collapsible.Content>
              </Collapsible.Root>
            {/each}
          </div>
        {/if}
      {/snippet}
      <Button variant="outline" size="sm" onclick={() => (showReports = !showReports)} disabled={reports.length === 0}>
        {showReports ? t('settings.hide') : t('settings.review')}
      </Button>
      <Button variant="outline" size="sm" onclick={copyReports} disabled={reports.length === 0}>
        {#if copied}<Check /> {t('conn.copied')}{:else}<Copy /> {t('settings.copy_json')}{/if}
      </Button>
    </SettingRow>
  </SettingGroup>

  <SettingGroup title={t('settings.group_danger')} danger>
    <SettingRow label={t('settings.clear_reports_label')} note={t('settings.clear_reports_hint')}>
      {#if confirmClear}
        <Button variant="destructive" size="sm" onclick={clearReports} disabled={clearing}>{t('settings.clear_reports_confirm')}</Button>
        <Button variant="ghost" size="sm" onclick={() => (confirmClear = false)} disabled={clearing}>{t('settings.cancel')}</Button>
      {:else}
        <Button variant="destructive" size="sm" disabled={reports.length === 0} onclick={() => (confirmClear = true)}>{t('settings.clear')}</Button>
      {/if}
    </SettingRow>
  </SettingGroup>
</div>
</div>
