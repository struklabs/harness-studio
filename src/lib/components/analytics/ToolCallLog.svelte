<script lang="ts">
  // The tool log (`analytics.db` `tool_calls`) in the request inspector: every tool call of every
  // client (Playground, command line, Developer API), the refused ones included. Compact table;
  // a row opens to show the arguments in full.
  import { SvelteSet } from 'svelte/reactivity';
  import { ChevronRight, ShieldAlert, Wrench } from '@lucide/svelte';
  import { t } from '$lib/i18n.svelte';
  import * as Table from '$lib/components/ui/table';
  import * as Empty from '$lib/components/ui/empty';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import NativeSelect from '$lib/components/dashboard/parts/NativeSelect.svelte';
  import SectionLoading from '$lib/components/common/SectionLoading.svelte';
  import { fetchToolLog, type ToolLogRow } from '$lib/tools-client';
  import { prettyArgs } from '$lib/mcp-client';
  import { fmtInt, fmtMs, fmtTime } from './anFormat';

  interface Props {
    /** Start of the selected range (null = everything). */
    fromMs: number | null;
    /** Changes when the parent wants a reload. */
    reloadKey: number;
  }

  let { fromMs, reloadKey }: Props = $props();

  const PAGE = 100;
  let tool = $state('');
  let client = $state('');
  let rows = $state<ToolLogRow[]>([]);
  let toolOptions = $state<string[]>([]);
  let clientOptions = $state<string[]>([]);
  let more = $state(false);
  let loading = $state(false);
  let error = $state<string | null>(null);
  const open = new SvelteSet<string>();

  function remember(page: ToolLogRow[]) {
    toolOptions = [...new Set([...toolOptions, ...page.map((r) => r.tool)])].sort();
    clientOptions = [...new Set([...clientOptions, ...page.map((r) => r.client)])].sort();
  }

  async function load(next = false) {
    loading = true;
    try {
      const before = next && rows.length ? rows[rows.length - 1].started_at_ms : null;
      const page = await fetchToolLog({
        tool: tool || null,
        client: client || null,
        from_ms: fromMs,
        before_ms: before,
        limit: PAGE,
      });
      rows = next ? [...rows, ...page] : page;
      more = page.length === PAGE;
      remember(page);
      error = null;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    // Reload whenever a filter, the range or the parent's refresh changes.
    void [tool, client, fromMs, reloadKey];
    void load();
  });

  function statusOf(r: ToolLogRow): { text: string; cls: string } {
    if (!r.error_code) return { text: t('an.tool_ok'), cls: 'text-success border-success/30 bg-success/10' };
    if (r.error_code === 'approval_required') return { text: t('an.tool_refused'), cls: 'text-warning border-warning/40 bg-warning/10' };
    return { text: r.error_code, cls: 'text-destructive border-destructive/40 bg-destructive/10' };
  }

  function toggle(id: string) {
    if (!open.delete(id)) open.add(id);
  }
</script>

<div class="flex flex-col gap-3 px-6 py-4">
  <div class="flex flex-wrap items-end gap-2">
    <label class="flex flex-col gap-1 text-xs text-muted-foreground">
      {t('an.tool_filter_tool')}
      <NativeSelect class="h-7 w-44 text-xs" value={tool} onchange={(e) => (tool = (e.currentTarget as HTMLSelectElement).value)}>
        <option value="">{t('an.filter_all')}</option>
        {#each toolOptions as v (v)}
          <option value={v}>{v}</option>
        {/each}
      </NativeSelect>
    </label>
    <label class="flex flex-col gap-1 text-xs text-muted-foreground">
      {t('an.filter_client')}
      <NativeSelect class="h-7 w-36 text-xs" value={client} onchange={(e) => (client = (e.currentTarget as HTMLSelectElement).value)}>
        <option value="">{t('an.filter_all')}</option>
        {#each clientOptions as v (v)}
          <option value={v}>{v}</option>
        {/each}
      </NativeSelect>
    </label>
    <span class="ml-auto pb-1 text-xs text-muted-foreground">{t('an.tool_shown', { n: fmtInt(rows.length) })}{more ? '+' : ''}</span>
  </div>
  <p class="text-xs text-muted-foreground">{t('an.tool_log_note')}</p>

  {#if error}
    <p class="text-xs text-destructive" role="alert">{t('an.load_failed', { error })}</p>
  {/if}

  {#if !loading && rows.length === 0 && !error}
    <Empty.Root class="py-16">
      <Empty.Header>
        <Empty.Media variant="icon"><Wrench /></Empty.Media>
        <Empty.Title>{t('an.tool_empty')}</Empty.Title>
        <Empty.Description>{t('an.tool_empty_desc')}</Empty.Description>
      </Empty.Header>
    </Empty.Root>
  {:else if loading && rows.length === 0 && !error}
    <SectionLoading shape="rows" rows={6} label={t('sl.req_label')} phases={[t('sl.req_1'), t('sl.req_2')]} />
  {:else}
    <div class="@container overflow-x-auto rounded-lg border border-border">
      <Table.Root>
        <Table.Header>
          <Table.Row class="bg-muted/30 hover:bg-muted/30">
            <Table.Head class="w-6"></Table.Head>
            <Table.Head class="text-xs">{t('an.col_time')}</Table.Head>
            <Table.Head class="text-xs">{t('an.tool_col_tool')}</Table.Head>
            <Table.Head class="hidden w-full text-xs @3xl:table-cell">{t('an.tool_col_args')}</Table.Head>
            <Table.Head class="hidden text-xs @5xl:table-cell">{t('an.tool_col_origin')}</Table.Head>
            <Table.Head class="text-xs">{t('an.col_client')}</Table.Head>
            <Table.Head class="text-xs">{t('an.col_status')}</Table.Head>
            <Table.Head class="text-right text-xs">{t('an.tool_col_duration')}</Table.Head>
            <Table.Head class="text-right text-xs">{t('an.tool_col_size')}</Table.Head>
          </Table.Row>
        </Table.Header>
        <Table.Body>
          {#each rows as r (r.id)}
            {@const s = statusOf(r)}
            {@const expanded = open.has(r.id)}
            <Table.Row class="cursor-pointer" onclick={() => toggle(r.id)} aria-expanded={expanded}>
              <Table.Cell class="py-1.5 pr-0">
                <ChevronRight class="size-3.5 text-muted-foreground transition-transform {expanded ? 'rotate-90' : ''}" aria-hidden="true" />
              </Table.Cell>
              <Table.Cell class="py-1.5 font-num text-xs whitespace-nowrap text-muted-foreground">{fmtTime(r.started_at_ms)}</Table.Cell>
              <Table.Cell class="py-1.5 font-mono text-xs whitespace-nowrap">
                <span class="inline-flex items-center gap-1">
                  {r.tool}
                  {#if r.class === 'dangerous'}<ShieldAlert class="size-3 text-warning" aria-label={t('tools.class_dangerous')} />{/if}
                </span>
              </Table.Cell>
              <Table.Cell class="hidden max-w-0 py-1.5 font-mono text-xs @3xl:table-cell" title={r.args}>
                <span class="block truncate">{r.args}</span>
              </Table.Cell>
              <Table.Cell class="hidden max-w-48 truncate py-1.5 text-xs @5xl:table-cell" title={[r.conversation, r.model].filter(Boolean).join(' · ')}>
                {r.model ?? '—'}
              </Table.Cell>
              <Table.Cell class="max-w-32 truncate py-1.5 text-xs">{r.client}</Table.Cell>
              <Table.Cell class="py-1.5">
                <span class="inline-flex items-center gap-1">
                  <Badge variant="outline" class="h-5 px-1.5 text-xs {s.cls}">{s.text}</Badge>
                  {#if r.approved}<span class="text-xs text-muted-foreground">{t('an.tool_approved')}</span>{/if}
                </span>
              </Table.Cell>
              <Table.Cell class="py-1.5 text-right font-num text-xs">{fmtMs(r.duration_ms)}</Table.Cell>
              <Table.Cell class="py-1.5 text-right font-num text-xs whitespace-nowrap">
                {fmtInt(r.result_chars)}{#if r.truncated}<span class="ml-1 text-warning">{t('an.tool_truncated')}</span>{/if}
              </Table.Cell>
            </Table.Row>
            {#if expanded}
              <Table.Row class="bg-muted/20 hover:bg-muted/20">
                <Table.Cell></Table.Cell>
                <Table.Cell colspan={8} class="py-2">
                  <div class="flex min-w-0 flex-col gap-1.5 text-xs">
                    <pre class="max-h-48 overflow-auto rounded-md border border-border bg-muted p-2 font-mono leading-relaxed break-words whitespace-pre-wrap select-text">{prettyArgs(r.args)}</pre>
                    <p class="text-muted-foreground">
                      {t('an.tool_chars', { n: fmtInt(r.result_chars) })}
                      {#if r.conversation}· {t('an.tool_conversation')}: <span class="font-mono">{r.conversation}</span>{/if}
                      {#if r.model}· {r.model}{/if}
                      {#if r.network_target}· {t('an.tool_network')}: <span class="font-mono">{r.network_target}</span>{/if}
                    </p>
                  </div>
                </Table.Cell>
              </Table.Row>
            {/if}
          {/each}
        </Table.Body>
      </Table.Root>
    </div>
    {#if more}
      <Button variant="outline" size="sm" class="self-center" disabled={loading} onclick={() => load(true)}>{t('an.load_more')}</Button>
    {/if}
  {/if}
</div>
