<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '$lib/ipc';
  import { Copy } from '@lucide/svelte';
  import { toast } from 'svelte-sonner';
  import type { RequestDetail } from '$lib/types';
  import { t, type TKey } from '$lib/i18n.svelte';
  import * as Sheet from '$lib/components/ui/sheet';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import StatRow from '$lib/components/perf/StatRow.svelte';
  import { NA, PHASE_COLORS, fmtBytes, fmtDateTime, fmtInt, fmtMs, fmtRate, fmtTps } from './anFormat';
  import { alsoClient, categoryLabel, linkedRequest, toolDiagnosis } from './toolDiag';
  import { findAgentModel, listAgentModels, TOOL_MIN_ROUNDS, type AgentModel } from '$lib/components/connections/agentModels';

  interface Props {
    inline?: boolean;
    id: string;
    onClose: () => void;
    /** Opens another request (the one that made the call a failed tool result answers). */
    onOpen?: ((id: string) => void) | null;
  }

  let { inline = false, id, onClose, onOpen = null }: Props = $props();

  let detail = $state<RequestDetail | null>(null);
  let error = $state<string | null>(null);

  onMount(() => {
    invoke<RequestDetail | null>('get_analytics_request', { id })
      .then((d) => {
        detail = d;
        if (!d) error = id;
      })
      .catch((e) => (error = String(e)));
  });

  let r = $derived(detail?.request ?? null);
  let diag = $derived(r ? toolDiagnosis(r) : null);
  let linked = $derived(diag ? linkedRequest(diag) : null);
  let alsoErr = $derived(diag ? alsoClient(diag) : null);

  // A request that offered tools to a model whose tool calls are not proven: say so on its own
  // line, whatever this one request's diagnosis was (one clean round does not make a model safe).
  let agentModel = $state<AgentModel | undefined>(undefined);
  $effect(() => {
    const alias = r && r.tools_offered > 0 ? (r.alias ?? r.model) : null;
    if (!alias) return;
    listAgentModels()
      .then((list) => (agentModel = findAgentModel(list, alias)))
      .catch(() => (agentModel = undefined));
  });
  let unverified = $derived(
    !!agentModel && agentModel.tool_level !== 'native' && agentModel.tools !== 'verified'
  );
  let unverifiedText = $derived.by(() => {
    const m = agentModel;
    if (!m) return '';
    const n = m.tool_rounds_ok + m.tool_rounds_bad;
    return m.tool_level
      ? t(`td.level_${m.tool_level}`, { ok: m.tool_rounds_ok, n })
      : t('td.level_unmeasured', { n, need: TOOL_MIN_ROUNDS });
  });
  let span = $derived(Math.max(1, r?.total_ms ?? 1, ...(detail?.timeline ?? []).map((p) => p.start_ms + p.duration_ms)));
  const phaseLabel: Record<string, TKey> = {
    load: 'an.phase_load',
    queue: 'an.phase_queue',
    prompt: 'an.phase_prompt',
    generation: 'an.phase_generation',
    other: 'an.phase_other',
  };
  const opt = (v: string | null | undefined) => (v ? v : null);
  const tokensWithCache = (p: number | null | undefined, c: number | null | undefined) =>
    p == null ? NA : c ? `${fmtInt(p)} (${t('an.f_cached')}: ${fmtInt(c)})` : fmtInt(p);

  async function copyId() {
    try {
      await navigator.clipboard.writeText(id);
      toast.success(t('an.copied'));
    } catch {
      // clipboard unavailable
    }
  }

  function pretty(text: string): string {
    try {
      return JSON.stringify(JSON.parse(text), null, 2);
    } catch {
      return text;
    }
  }
</script>

{#snippet content()}
  <header class="flex items-center justify-between gap-2 border-b border-border p-4">
    <div><h2 class="font-num text-base font-semibold">{t('an.detail_title', { id })}</h2><p class="text-xs text-muted-foreground">{r ? fmtDateTime(r.started_at_ms) : ''}</p></div>
    <Button variant="ghost" size="icon-sm" onclick={copyId} aria-label={t('an.copy_id')} title={t('an.copy_id')}><Copy /></Button>
  </header>
    {#if error}
      <p class="p-4 text-xs text-destructive">{t('an.load_failed', { error })}</p>
    {:else if detail && r}
      <div class="flex flex-col gap-5 p-4">
        <section class="flex flex-col gap-2">
          <h3 class="text-xs font-semibold">{t('an.timeline')}</h3>
          <div class="relative flex h-6 w-full overflow-hidden rounded-md bg-muted">
            {#each detail.timeline as p, i (i)}
              <div
                class="h-full {PHASE_COLORS[p.kind] ?? 'bg-muted-foreground/40'}"
                style="margin-left: {i === 0 ? (p.start_ms / span) * 100 : 0}%; width: {Math.max(0.5, (p.duration_ms / span) * 100)}%"
                title="{t(phaseLabel[p.kind] ?? 'an.phase_other')}: {fmtMs(p.duration_ms)}"
              ></div>
            {/each}
            {#if r.ttft_ms != null}
              <div class="absolute top-0 h-full w-0.5 bg-foreground" style="left: {Math.min(100, (r.ttft_ms / span) * 100)}%" title={t('an.ttft_marker', { ms: fmtMs(r.ttft_ms) })}></div>
            {/if}
          </div>
          <div class="flex flex-wrap gap-x-4 gap-y-1 text-xs">
            {#each detail.timeline as p, i (i)}
              <span class="flex items-center gap-1.5">
                <span class="size-2 rounded-sm {PHASE_COLORS[p.kind]}"></span>
                {t(phaseLabel[p.kind] ?? 'an.phase_other')}
                <span class="font-num text-muted-foreground">{fmtMs(p.duration_ms)}</span>
              </span>
            {/each}
            {#if r.ttft_ms != null}
              <span class="flex items-center gap-1.5">
                <span class="h-2 w-0.5 bg-foreground"></span>
                {t('an.ttft_marker', { ms: fmtMs(r.ttft_ms) })}
              </span>
            {/if}
          </div>
          <p class="text-xs text-muted-foreground">{t('an.timeline_hint')}</p>
        </section>

        {#if r.error_code || r.oom || r.cancelled || (r.status >= 400 && r.status !== 499)}
          <section class="flex flex-col gap-1 rounded-md border border-destructive/30 bg-destructive/5 p-3">
            <h3 class="text-xs font-semibold text-destructive">{t('an.section_error')}</h3>
            <StatRow label={t('an.f_status')} value={String(r.status)} />
            {#if r.error_code}<StatRow label={t('an.f_error_code')} value={r.error_code} />{/if}
            {#if r.oom}<p class="text-xs text-destructive">{t('an.f_oom')}</p>{/if}
            {#if r.cancelled}<p class="text-xs text-muted-foreground">{t('an.f_cancelled')}</p>{/if}
          </section>
        {/if}

        {#if unverified}
          <p class="rounded-md border border-warning/40 bg-warning/5 px-3 py-2 text-xs" role="note">
            <span class="font-medium">{t('td.unverified')}</span>
            <span class="text-muted-foreground"> {unverifiedText}</span>
          </p>
        {/if}

        {#if diag}
          <section class="flex flex-col gap-1.5 rounded-md border border-warning/40 bg-warning/5 p-3">
            <h3 class="text-xs font-semibold">{t('td.section')}</h3>
            <p class="text-xs font-medium">{diag.title}</p>
            <p class="text-xs">{diag.suggestion}</p>
            <div class="mt-1 flex flex-col divide-y divide-border/60">
              <StatRow label={t('td.f_category')} value={categoryLabel(diag.category)} />
              <StatRow label={t('td.f_finding')} value={diag.code} />
              {#if diag.detail.recovered === true}<StatRow label={t('td.f_recovered')} value={String(diag.detail.recovered_calls ?? 0)} />{/if}
              <StatRow label={t('td.f_tools')} value={`${r.tools_offered} / ${r.tool_calls}`} />
              {#if typeof diag.detail.tool === 'string'}<StatRow label={t('td.f_tool')} value={diag.detail.tool} />{/if}
              {#if typeof diag.detail.path === 'string' && diag.detail.path}<StatRow label={t('td.f_path')} value={diag.detail.path} />{/if}
              {#if typeof diag.detail.rule === 'string'}<StatRow label={t('td.f_rule')} value={diag.detail.rule} />{/if}
            </div>
            {#if typeof diag.detail.excerpt === 'string' && diag.detail.excerpt}
              <span class="text-xs text-muted-foreground">{t('td.f_excerpt')}</span>
              <code class="max-h-24 overflow-auto rounded bg-muted px-2 py-1 font-mono text-xs break-all whitespace-pre-wrap">{diag.detail.excerpt}</code>
            {/if}
            {#if alsoErr}<p class="text-xs text-muted-foreground">{t('td.also_client')}</p>{/if}
            {#if linked}
              <div class="flex items-center gap-2 text-xs">
                <span class="text-muted-foreground">{t('td.f_linked')}</span>
                <span class="font-num">{linked}</span>
                {#if onOpen}<Button size="sm" variant="outline" class="h-6 px-2 text-xs" onclick={() => onOpen?.(linked)}>{t('td.open_linked')}</Button>{/if}
              </div>
            {/if}
            <p class="text-xs text-muted-foreground">{t('td.section_hint')}</p>
          </section>
        {/if}

        <section class="flex flex-col divide-y divide-border/60">
          <h3 class="pb-1 text-xs font-semibold">{t('an.section_request')}</h3>
          <StatRow label={t('an.f_model')} value={opt(r.alias)} />
          {#if r.model && r.model !== r.alias}<StatRow label={t('an.f_requested')} value={r.model} />{/if}
          <StatRow label={t('an.f_endpoint')} value={`${r.endpoint}${r.stream ? ` · ${t('an.stream')}` : ''}`} />
          <StatRow label={t('an.f_client')} value={opt(r.client)} />
          <StatRow label={t('an.f_user_agent')} value={opt(r.user_agent)} />
          <StatRow label={t('an.f_harness_client')} value={opt(r.harness_client)} />
          <StatRow label={t('an.f_token')} value={opt(r.token_name)} />
          <StatRow label={t('an.f_server')} value={opt(r.server_id)} />
          <StatRow label={t('an.f_status')} value={String(r.status)} />
          <StatRow label={t('an.f_finish')} value={opt(r.finish_reason)} />
          {#if r.evicted.length}<StatRow label={t('an.f_evicted')} value={r.evicted.join(', ')} />{/if}
          {#if r.compat.length}<StatRow label={t('an.f_compat')} value={r.compat.join('; ')} />{/if}
        </section>

        <section class="flex flex-col divide-y divide-border/60">
          <h3 class="pb-1 text-xs font-semibold">{t('an.section_tokens')}</h3>
          <StatRow label={t('an.f_prompt')} value={tokensWithCache(r.prompt_tokens, r.cached_tokens)} />
          <StatRow label={t('an.f_completion')} value={r.completion_tokens == null ? null : fmtInt(r.completion_tokens)} />
          <StatRow label={t('an.f_prompt_speed')} value={r.prompt_per_second == null ? null : fmtTps(r.prompt_per_second)} />
          <StatRow label={t('an.f_gen_speed')} value={r.predicted_per_second == null ? null : fmtTps(r.predicted_per_second)} />
          <StatRow label={t('an.f_ttft')} value={r.ttft_ms == null ? null : fmtMs(r.ttft_ms)} />
          <StatRow label={t('an.f_total')} value={fmtMs(r.total_ms)} />
          <StatRow label={t('an.f_queue')} value={fmtMs(r.queue_wait_ms)} />
          <StatRow label={t('an.f_load')} value={r.load_ms == null ? null : fmtMs(r.load_ms)} />
          <StatRow
            label={t('an.f_context')}
            value={r.context_fill == null ? null : `${fmtRate(r.context_fill)} (${fmtInt((r.prompt_tokens ?? 0) + (r.completion_tokens ?? 0))} / ${fmtInt(r.ctx_size)})`}
          />
          <StatRow
            label={t('an.f_spec')}
            value={detail.spec_accept_rate == null ? null : `${fmtRate(detail.spec_accept_rate)} (${fmtInt(r.draft_accepted)} / ${fmtInt(r.draft_n)})`}
          />
        </section>

        <section class="flex flex-col divide-y divide-border/60">
          <h3 class="pb-1 text-xs font-semibold">{t('an.section_resources')}</h3>
          <StatRow label={t('an.f_vram')} value={r.vram_peak_bytes == null ? null : fmtBytes(r.vram_peak_bytes)} hint={t('an.vram_hint')} />
          <StatRow label={t('an.f_ram')} value={r.ram_peak_bytes == null ? null : fmtBytes(r.ram_peak_bytes)} />
          <StatRow label={t('an.f_backend')} value={opt(r.backend)} />
        </section>

        <section class="flex flex-col gap-2">
          <h3 class="text-xs font-semibold">{t('an.content')}</h3>
          {#if detail.content}
            {@const c = detail.content}
            <p class="text-xs text-muted-foreground">{t('an.content_expires', { date: fmtDateTime(c.expires_at_ms) })}</p>
            {#each [
              { label: t('an.content_request'), text: pretty(c.request), cut: c.request_truncated },
              { label: t('an.content_response'), text: pretty(c.response), cut: c.response_truncated },
            ] as block (block.label)}
              <details class="rounded-md border border-border">
                <summary class="cursor-pointer px-3 py-1.5 text-xs font-medium">
                  {block.label}
                  {#if block.cut}<Badge variant="outline" class="ml-2 h-4 px-1 text-xs">{t('an.content_truncated')}</Badge>{/if}
                </summary>
                <pre class="max-h-80 overflow-auto border-t border-border bg-muted/30 p-3 font-mono text-xs leading-snug whitespace-pre-wrap break-all">{block.text}</pre>
              </details>
            {/each}
          {:else}
            <p class="text-xs text-muted-foreground">{r.content_captured ? t('an.content_expired') : t('an.content_none')}</p>
          {/if}
        </section>
      </div>
    {:else}
      <p class="p-4 text-xs text-muted-foreground">{t('an.loading')}</p>
    {/if}
{/snippet}
{#if inline}
  <section>{@render content()}</section>
{:else}
  <Sheet.Root open={true} onOpenChange={(o) => !o && onClose()}>
    <Sheet.Content side="right" class="w-[min(640px,94vw)] gap-0 overflow-y-auto p-0 data-[side=right]:sm:max-w-none">
      <Sheet.Title class="sr-only">{t('an.detail_title', { id })}</Sheet.Title><Sheet.Description class="sr-only">{r ? fmtDateTime(r.started_at_ms) : ''}</Sheet.Description>
      {@render content()}
    </Sheet.Content>
  </Sheet.Root>
{/if}
