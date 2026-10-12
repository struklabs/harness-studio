<script lang="ts">
  import { uiLevel } from '$lib/stores/uiLevel.svelte';
  import HelpPopover from '../common/HelpPopover.svelte';
  // Settings > OpenTelemetry: OTLP export of gateway traces, logs and metrics (off by default).
  // Saved through its own command (validated in the core, header values to the keychain), not
  // the general settings form. "Test export" sends one real span with the form's values.
  import { onDestroy, onMount } from 'svelte';
  import { invoke } from '$lib/ipc';
  import { toast } from 'svelte-sonner';
  import { CircleCheck, CircleX, Send } from '@lucide/svelte';
  import type { OtelOverview, OtelSettings, OtelTestResult } from '$lib/types';
  import { t, locale } from '$lib/i18n.svelte';
  import { localizeError } from '$lib/network';
  import { windowActive } from '$lib/windowActivity';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Switch } from '$lib/components/ui/switch';
  import { Textarea } from '$lib/components/ui/textarea';
  import NativeSelect from '$lib/components/dashboard/parts/NativeSelect.svelte';
  import SettingGroup from './SettingGroup.svelte';
  import SettingRow from './SettingRow.svelte';
  import NumberField from './NumberField.svelte';

  let overview = $state<OtelOverview | null>(null);
  let draft = $state<OtelSettings | null>(null);
  /** New header lines; empty = keep the saved headers. */
  let headersText = $state('');
  let clearHeaders = $state(false);
  let busy = $state(false);
  let testing = $state(false);
  let error = $state<string | null>(null);
  let test = $state<OtelTestResult | null>(null);
  let timer: ReturnType<typeof setInterval> | undefined;

  const DEFAULTS = { http_protobuf: 'http://127.0.0.1:4318', grpc: 'http://127.0.0.1:4317' } as const;

  function adopt(o: OtelOverview) {
    overview = o;
    draft = structuredClone($state.snapshot(o.settings) as OtelSettings);
  }

  async function refresh(keepDraft = false) {
    try {
      const o = await invoke<OtelOverview>('get_otel_settings');
      if (keepDraft && draft) overview = o;
      else adopt(o);
    } catch (e) {
      error = localizeError(e);
    }
  }

  onMount(() => {
    refresh();
    // Counters move while export runs; poll only while the window is in front.
    timer = setInterval(() => {
      if (windowActive() && overview?.status.active) refresh(true);
    }, 5000);
  });
  onDestroy(() => clearInterval(timer));

  let headersChange = $derived<string | null>(clearHeaders ? '' : headersText.trim() ? headersText : null);

  let dirty = $derived(
    !!overview &&
      !!draft &&
      (JSON.stringify($state.snapshot(draft)) !== JSON.stringify(overview.settings) || headersChange !== null),
  );

  function setProtocol(p: OtelSettings['protocol']) {
    if (!draft) return;
    // Follow the protocol's default port while the address is still a default one.
    if (Object.values(DEFAULTS).includes(draft.endpoint as (typeof DEFAULTS)[keyof typeof DEFAULTS])) {
      draft.endpoint = DEFAULTS[p];
    }
    draft.protocol = p;
  }

  async function save() {
    if (!draft) return;
    busy = true;
    error = null;
    try {
      adopt(
        await invoke<OtelOverview>('save_otel_settings', {
          settings: $state.snapshot(draft),
          headers: headersChange,
        }),
      );
      headersText = '';
      clearHeaders = false;
      toast.success(t('otelui.saved'));
    } catch (e) {
      error = localizeError(e);
    } finally {
      busy = false;
    }
  }

  async function runTest() {
    if (!draft) return;
    testing = true;
    test = null;
    error = null;
    try {
      test = await invoke<OtelTestResult>('test_otel_export', {
        settings: { ...$state.snapshot(draft), enabled: true },
        headers: headersChange,
      });
    } catch (e) {
      error = localizeError(e);
    } finally {
      testing = false;
    }
  }

  const fmtTime = (ms: number) => new Date(ms).toLocaleString(locale());
  const existingHelpId = $props.id();
</script>

<div class="flex flex-col gap-6">
  {#if overview && !overview.status.built}
    <p class="rounded-md border border-warning/40 bg-warning/5 p-3 text-xs" role="status">{t('otelui.not_built')}</p>
  {/if}

  <SettingGroup title={t('otelui.export_title')} hint={t('otelui.export_desc')}>
    {#snippet actions()}
      {#if overview?.status.active}
        <Badge variant="outline" class="text-success">{t('otelui.status_active')}</Badge>
      {:else}
        <Badge variant="outline">{t('otelui.status_off')}</Badge>
      {/if}
    {/snippet}
    {#if draft}
      <SettingRow label={t('otelui.enable')} hint={t('otelui.enable_hint')} for="otel-enabled">
        <Switch id="otel-enabled" bind:checked={draft.enabled} />
      </SettingRow>
      <SettingRow help={t('help.otel_protocol')} label={t('otelui.protocol')} for="otel-protocol">
        <NativeSelect
          id="otel-protocol"
          class="w-56"
          value={draft.protocol}
          onchange={(e) => setProtocol(e.currentTarget.value as OtelSettings['protocol'])}
        >
          <option value="http_protobuf">{t('otelui.protocol_http')}</option>
          <!-- gRPC only in builds with `otel-grpc`; a stored gRPC choice stays visible (disabled). -->
          {#if overview?.status.grpc}
            <option value="grpc">{t('otelui.protocol_grpc')}</option>
          {:else if draft.protocol === 'grpc'}
            <option value="grpc" disabled>{t('otelui.protocol_grpc_unavailable')}</option>
          {/if}
        </NativeSelect>
      </SettingRow>
      {#if overview?.status.built && !overview.status.grpc && draft.protocol === 'grpc'}
        <p class="mx-4 rounded-md border border-warning/40 bg-warning/5 p-3 text-xs" role="status">
          {t('otelui.grpc_not_built')}
        </p>
      {/if}
      <SettingRow label={t('otelui.endpoint')} hint={draft.protocol === 'grpc' ? t('otelui.endpoint_hint_grpc') : t('otelui.endpoint_hint_http')} for="otel-endpoint">
<Input
          id="otel-endpoint"
          class="h-8 font-mono text-xs"
          spellcheck={false}
          autocomplete="off"
          placeholder={DEFAULTS[draft.protocol]}
          bind:value={draft.endpoint}
        />
      </SettingRow>
      <SettingRow label={t('otelui.service')} hint={t('otelui.service_hint')} for="otel-service">
        <Input id="otel-service" class="h-8 w-48 text-xs" spellcheck={false} autocomplete="off" bind:value={draft.service_name} />
      </SettingRow>
      <SettingRow label={t('otelui.traces')} hint={t('otelui.traces_hint')} for="otel-traces">
        <Switch id="otel-traces" bind:checked={draft.traces} />
      </SettingRow>
      <SettingRow label={t('otelui.logs')} hint={t('otelui.logs_hint')} for="otel-logs">
        <Switch id="otel-logs" bind:checked={draft.logs} />
      </SettingRow>
      <SettingRow label={t('otelui.metrics')} hint={t('otelui.metrics_hint')} for="otel-metrics">
        <Switch id="otel-metrics" bind:checked={draft.metrics} />
      </SettingRow>
      {#if draft.metrics}
        <SettingRow help={t('help.otel_interval')} label={t('otelui.interval')} for="otel-interval">
          <NumberField id="otel-interval" value={draft.metrics_interval_secs} min={10} onchange={(v) => draft && (draft.metrics_interval_secs = v)} />
        </SettingRow>
      {/if}
      <SettingRow label={t('otelui.content')} note={t('otelui.content_hint')} help={t('otelui.content_help')} for="otel-content">
        <Switch id="otel-content" bind:checked={draft.include_content} />
      </SettingRow>
      <div class="flex flex-col gap-1.5 px-4 py-3">
        <label for="otel-headers" class="text-sm font-medium">{t('otelui.headers')}</label>
        <div class="flex min-w-0 flex-wrap items-center gap-2 text-xs">
          {#if draft.header_names.length && !clearHeaders}
            <span class="min-w-0 truncate text-muted-foreground">
              {t('otelui.headers_saved', { names: draft.header_names.join(', ') })}
            </span>
            <Button variant="ghost" size="sm" onclick={() => ((clearHeaders = true), (headersText = ''))}>
              {t('otelui.headers_clear')}
            </Button>
          {:else if clearHeaders}
            <span class="text-warning">{t('otelui.headers_clear_pending')}</span>
            <Button variant="ghost" size="sm" onclick={() => (clearHeaders = false)}>{t('settings.cancel')}</Button>
          {:else}
            <span class="text-muted-foreground">{t('otelui.headers_none')}</span>
          {/if}
        </div>
        <Textarea aria-describedby={existingHelpId + '-existing-otelui-headers_hint'}
          id="otel-headers"
          rows={2}
          class="font-mono text-xs"
          spellcheck={false}
          autocomplete="off"
          disabled={clearHeaders}
          placeholder={t('otelui.headers_placeholder')}
          bind:value={headersText}
        />
        {#if uiLevel.advanced}<HelpPopover text={t('otelui.headers_hint')} />{:else}<p class="text-xs text-muted-foreground">{t('otelui.headers_hint')}</p>{/if}<span id={existingHelpId + '-existing-otelui-headers_hint'} class="sr-only" data-field-help-text>{t('otelui.headers_hint')}</span>
      </div>
      <div class="flex flex-col gap-2 px-4 py-3">
        <div class="flex flex-wrap items-center justify-between gap-2">
          {#if uiLevel.advanced}<HelpPopover text={t('otelui.test_hint')} />{:else}<p class="text-xs text-muted-foreground">{t('otelui.test_hint')}</p>{/if}
          <div class="flex shrink-0 flex-wrap gap-2">
            <Button variant="outline" size="sm" disabled={testing || !overview?.status.built} onclick={runTest}>
              <Send />
              {testing ? t('otelui.testing') : t('otelui.test')}
            </Button>
            <Button size="sm" disabled={busy || !dirty} onclick={save}>{t('otelui.save')}</Button>
          </div>
        </div>
        {#if test}
          <p
            class="flex min-w-0 items-start gap-1.5 text-xs break-all {test.ok ? 'text-success' : 'text-destructive'}"
            role="status"
          >
            {#if test.ok}
              <CircleCheck class="mt-px size-3.5 shrink-0" />
              {t('otelui.test_ok', { url: test.url, ms: test.elapsed_ms, trace: test.trace_id })}
            {:else}
              <CircleX class="mt-px size-3.5 shrink-0" />
              {t('otelui.test_failed', { url: test.url, error: localizeError(test.error ?? '') })}
            {/if}
          </p>
        {/if}
        {#if error}<p class="text-xs text-destructive" role="alert">{error}</p>{/if}
      </div>
    {/if}
  </SettingGroup>

  {#if overview?.status.active || overview?.status.spans_exported || overview?.status.logs_exported || overview?.status.last_error}
    <SettingGroup title={t('otelui.status_title')}>
      <div class="flex flex-col gap-1 px-4 py-3 text-xs">
        <p class="tabular">
          {t('otelui.status_counts', {
            exported: overview.status.spans_exported,
            dropped: overview.status.spans_dropped,
            logs: overview.status.logs_exported,
            logsDropped: overview.status.logs_dropped,
            errors: overview.status.export_errors,
          })}
        </p>
        {#if overview.status.last_export_ms}
          <p class="text-muted-foreground">{t('otelui.status_last_export', { time: fmtTime(overview.status.last_export_ms) })}</p>
        {/if}
        {#if overview.status.last_error}
          <p class="break-all text-destructive">{t('otelui.status_last_error', { error: localizeError(overview.status.last_error) })}</p>
        {/if}
      </div>
    </SettingGroup>
  {/if}
</div>
