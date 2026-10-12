<script lang="ts">
  import { untrack } from 'svelte';
  import { invoke } from '$lib/ipc';
  import { ListTree } from '@lucide/svelte';
  import type { RequestPage, RequestQuery, RequestRow } from '$lib/types';
  import { t } from '$lib/i18n.svelte';
  import * as Table from '$lib/components/ui/table';
  import * as Empty from '$lib/components/ui/empty';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import NativeSelect from '$lib/components/dashboard/parts/NativeSelect.svelte';
  import RequestDetailSheet from './RequestDetailSheet.svelte';
  import SectionLoading from '$lib/components/common/SectionLoading.svelte';
  import { fmtInt, fmtMs, fmtTime, fmtTps } from './anFormat';
  import { categoryLabel, toolDiagnosis, type ToolCategory } from './toolDiag';

  interface Props {
    /** Start of the selected range (null = everything). */
    onSelect?: (id: string) => void;
    fromMs: number | null;
    /** Changes when the parent wants a reload. */
    reloadKey: number;
    /** Changes when a gateway request finished: the newest rows are added in place. */
    liveKey: number;
  }

  let { onSelect, fromMs, reloadKey, liveKey }: Props = $props();

  const PAGE = 100;
  let model = $state('');
  let client = $state('');
  let endpoint = $state('');
  let status = $state('');
  /** Tool-call diagnosis filter: '' | 'issue' | 'clean' | a category. */
  let tool = $state('');
  let rows = $state<RequestRow[]>([]);
  let total = $state(0);
  let options = $state<{ models: string[]; clients: string[]; endpoints: string[] }>({ models: [], clients: [], endpoints: [] });
  let loading = $state(false);
  let error = $state<string | null>(null);
  let selected = $state<string | null>(null);

  function query(before: number | null): RequestQuery {
    return {
      model: model || null,
      client: client || null,
      endpoint: endpoint || null,
      status: status || null,
      tool_issue: tool === 'issue' ? true : tool === 'clean' ? false : null,
      tool_category: tool && tool !== 'issue' && tool !== 'clean' ? tool : null,
      from_ms: fromMs,
      before_ms: before,
      limit: PAGE,
    };
  }

  async function load(more = false) {
    loading = true;
    try {
      const before = more && rows.length ? rows[rows.length - 1].started_at_ms : null;
      const page = await invoke<RequestPage>('list_analytics_requests', { query: query(before) });
      rows = more ? [...rows, ...page.requests] : page.requests;
      total = page.total;
      options = { models: page.models, clients: page.clients, endpoints: page.endpoints };
      error = null;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  /**
   * Adds what arrived since the last read: reads the newest page and keeps every older row
   * already shown (pages loaded with "load more"), so the list does not jump back. Filters
   * and range are the ones on screen; an open detail panel is not touched.
   */
  async function refreshLive() {
    try {
      const page = await invoke<RequestPage>('list_analytics_requests', { query: query(null) });
      const fresh = new Set(page.requests.map((r) => r.id));
      const oldest = page.requests.length === PAGE ? page.requests[page.requests.length - 1].started_at_ms : null;
      // A full page may not reach back to the rows already shown: keep those older than it.
      const older = oldest == null ? [] : rows.filter((r) => !fresh.has(r.id) && r.started_at_ms <= oldest);
      rows = [...page.requests, ...older];
      total = page.total;
      options = { models: page.models, clients: page.clients, endpoints: page.endpoints };
      error = null;
    } catch {
      // The next push or the refresh button tries again.
    }
  }

  // Deliberately the value at mount: only later pushes reload.
  let lastLive = untrack(() => liveKey);
  $effect(() => {
    if (liveKey === lastLive) return;
    lastLive = liveKey;
    void untrack(refreshLive);
  });

  $effect(() => {
    // Reload whenever a filter, the range or the parent's refresh changes.
    void [model, client, endpoint, status, tool, fromMs, reloadKey];
    void load();
  });

  function statusOf(r: RequestRow): { text: string; cls: string } {
    if (r.oom) return { text: `OOM ${r.status}`, cls: 'text-destructive border-destructive/40 bg-destructive/10' };
    if (r.error_code || (r.status >= 400 && r.status !== 499))
      return { text: `${r.status} ${r.error_code ?? ''}`.trim(), cls: 'text-destructive border-destructive/40 bg-destructive/10' };
    if (r.cancelled) return { text: t('an.cancelled'), cls: 'text-warning border-warning/40 bg-warning/10' };
    return { text: String(r.status), cls: 'text-success border-success/30 bg-success/10' };
  }
</script>

<div class="flex flex-col gap-3 px-6 py-4">
  <div class="flex flex-wrap items-end gap-2">
    {#each [
      { label: t('an.filter_model'), get: () => model, set: (v: string) => (model = v), values: options.models },
      { label: t('an.filter_client'), get: () => client, set: (v: string) => (client = v), values: options.clients },
      { label: t('an.filter_endpoint'), get: () => endpoint, set: (v: string) => (endpoint = v), values: options.endpoints },
    ] as f (f.label)}
      <label class="flex flex-col gap-1 text-xs text-muted-foreground">
        {f.label}
        <NativeSelect class="h-7 w-44 text-xs" title={f.get() || undefined} value={f.get()} onchange={(e) => f.set((e.currentTarget as HTMLSelectElement).value)}>
          <option value="">{t('an.filter_all')}</option>
          {#each f.values as v (v)}
            <option value={v}>{v}</option>
          {/each}
        </NativeSelect>
      </label>
    {/each}
    <label class="flex flex-col gap-1 text-xs text-muted-foreground">
      {t('an.filter_status')}
      <NativeSelect class="h-7 w-36 text-xs" bind:value={status}>
        <option value="">{t('an.filter_all')}</option>
        <option value="ok">{t('an.status_ok')}</option>
        <option value="error">{t('an.status_error')}</option>
        <option value="oom">{t('an.status_oom')}</option>
        <option value="cancelled">{t('an.status_cancelled')}</option>
      </NativeSelect>
    </label>
    <label class="flex flex-col gap-1 text-xs text-muted-foreground">
      {t('td.filter')}
      <NativeSelect class="h-7 w-48 text-xs" bind:value={tool}>
        <option value="">{t('an.filter_all')}</option>
        <option value="issue">{t('td.filter_issue')}</option>
        <option value="clean">{t('td.filter_clean')}</option>
        <option value="runtime">{t('td.filter_runtime')}</option>
        <option value="context">{t('td.filter_context')}</option>
        <option value="model">{t('td.filter_model')}</option>
        <option value="client">{t('td.filter_client')}</option>
      </NativeSelect>
    </label>
    <span class="ml-auto pb-1 text-xs text-muted-foreground">{t('an.shown', { n: fmtInt(rows.length), total: fmtInt(total) })}</span>
  </div>

  {#if error}
    <p class="text-xs text-destructive" role="alert">{t('an.load_failed', { error })}</p>
  {/if}

  {#if !loading && rows.length === 0 && !error}
    <Empty.Root class="py-16">
      <Empty.Header>
        <Empty.Media variant="icon"><ListTree /></Empty.Media>
        <Empty.Title>{t('an.no_requests')}</Empty.Title>
        <Empty.Description>{t('an.empty_desc')}</Empty.Description>
      </Empty.Header>
    </Empty.Root>
  {:else if loading && rows.length === 0 && !error}
    <SectionLoading shape="rows" rows={8} label={t('sl.req_label')} phases={[t('sl.req_1'), t('sl.req_2')]} />
  {:else}
    <!-- Container query: in a narrow window TTFT moves to the request detail, so the model
         names get the room instead of the table scrolling sideways. -->
    <div class="@container overflow-x-auto rounded-lg border border-border">
      <Table.Root>
        <Table.Header>
          <Table.Row class="bg-muted/30 hover:bg-muted/30">
            <Table.Head class="text-xs">{t('an.col_time')}</Table.Head>
            <Table.Head class="w-full text-xs">{t('an.col_model')}</Table.Head>
            <Table.Head class="text-xs">{t('an.col_client')}</Table.Head>
            <Table.Head class="text-xs">{t('an.col_endpoint')}</Table.Head>
            <Table.Head class="text-xs">{t('an.col_status')}</Table.Head>
            <Table.Head class="text-right text-xs">{t('an.col_tokens')}</Table.Head>
            <Table.Head class="hidden text-right text-xs @5xl:table-cell">TTFT</Table.Head>
            <Table.Head class="text-right text-xs">{t('an.col_total')}</Table.Head>
            <Table.Head class="text-right text-xs">{t('an.col_gen_tps')}</Table.Head>
          </Table.Row>
        </Table.Header>
        <Table.Body>
          {#each rows as r (r.id)}
            {@const s = statusOf(r)}
            {@const diag = toolDiagnosis(r)}
            <Table.Row class="cursor-pointer" onclick={() => onSelect ? onSelect(r.id) : (selected = r.id)}>
              <Table.Cell class="py-1.5 font-num text-xs whitespace-nowrap text-muted-foreground">{fmtTime(r.started_at_ms)}</Table.Cell>
              <!-- Model names are the column that matters: they wrap (at "-", "/", ".") instead of being cut. -->
              <Table.Cell class="min-w-48 py-1.5 text-xs font-medium whitespace-normal [overflow-wrap:anywhere]" title={[...new Set([r.alias, r.model].filter(Boolean))].join(' · ')}>{r.alias ?? r.model ?? '—'}{#if diag}
                <!-- One line: who is at fault and what to do (the full text is in the detail panel). -->
                <span class="mt-0.5 block text-xs font-normal text-warning" title={diag.suggestion}>
                  {categoryLabel(diag.category as ToolCategory)}: {diag.title}
                </span>
              {/if}</Table.Cell>
              <Table.Cell class="max-w-32 truncate py-1.5 text-xs" title={[r.client, r.user_agent].filter(Boolean).join(' · ')}>{r.client ?? '—'}</Table.Cell>
              <Table.Cell class="py-1.5 font-mono text-xs whitespace-nowrap">
                {r.endpoint}
                {#if r.stream}<span class="block text-muted-foreground @5xl:ml-1 @5xl:inline"><span class="hidden @5xl:inline">· </span>{t('an.stream')}</span>{/if}
              </Table.Cell>
              <Table.Cell class="py-1.5">
                <Badge variant="outline" class="h-5 px-1.5 font-num text-xs {s.cls}">{s.text}</Badge>
              </Table.Cell>
              <Table.Cell class="py-1.5 text-right font-num text-xs">{fmtInt(r.prompt_tokens)} / {fmtInt(r.completion_tokens)}</Table.Cell>
              <Table.Cell class="hidden py-1.5 text-right font-num text-xs @5xl:table-cell">{fmtMs(r.ttft_ms)}</Table.Cell>
              <Table.Cell class="py-1.5 text-right font-num text-xs">{fmtMs(r.total_ms)}</Table.Cell>
              <Table.Cell class="py-1.5 text-right font-num text-xs">{fmtTps(r.predicted_per_second)}</Table.Cell>
            </Table.Row>
          {/each}
        </Table.Body>
      </Table.Root>
    </div>
    {#if rows.length < total}
      <Button variant="outline" size="sm" class="self-center" disabled={loading} onclick={() => load(true)}>{t('an.load_more')}</Button>
    {/if}
  {/if}
</div>

{#if selected}
  {#key selected}
    <RequestDetailSheet id={selected} onClose={() => (selected = null)} onOpen={(id) => (selected = id)} />
  {/key}
{/if}
