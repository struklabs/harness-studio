<script lang="ts">
  // Performance > Model Scores: what was measured per model on this device (load time,
  // benchmark speed, image and audio speed, tool-call probe) and the four scores built from it.
  import { invoke } from '#lib/ipc.js';
  import { toast } from 'svelte-sonner';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { library } from '#lib/stores/library.svelte.js';
  import { modelPerf } from '#lib/stores/modelPerf.svelte.js';
  import { defaultLoadFit } from '#lib/fit.js';
  import { baseName } from '#lib/stores/domain.svelte.js';
  import { modelKeyOfFile, paramsFromName, scoreModel, type ModelPerfRecord, type ScoreTask } from '#lib/modelScores.js';
  import type { FitCategory } from '#lib/types.js';
  import ScoreBreakdown from './common/ScoreBreakdown.svelte';
  import { Button } from '#lib/components/ui/button/index.js';
  import * as Table from '#lib/components/ui/table/index.js';
  import { LoaderCircle, ShieldCheck } from '@lucide/svelte';

  let { visible = true }: { visible?: boolean } = $props();

  const TASKS: { id: ScoreTask; label: TKey }[] = [
    { id: 'chat', label: 'ms.task_chat' },
    { id: 'tools', label: 'ms.task_tools' },
    { id: 'vision', label: 'ms.task_vision' },
    { id: 'image', label: 'ms.task_image' },
    { id: 'speech', label: 'ms.task_speech' },
  ];
  let task = $state<ScoreTask>('chat');

  $effect(() => {
    if (!visible) return;
    void modelPerf.load(true);
    void library.refresh();
  });

  // Hardware fit of the local file of each record (the estimate a default load would use).
  let fits = $state<Record<string, FitCategory>>({});
  $effect(() => {
    const files = library.models.filter((m) => m.is_ready && m.kind === 'llm');
    for (const r of modelPerf.records) {
      const file = files.find((m) => modelKeyOfFile(m.filename) === r.model_key);
      if (!file || fits[r.model_key]) continue;
      void defaultLoadFit({ model_path: file.path, model_size_bytes: file.size_bytes, context_size: 8192, gpu_layers: 999 })
        .then((fit) => { fits = { ...fits, [r.model_key]: fit.category }; })
        .catch(() => {});
    }
  });

  function kindOf(r: ModelPerfRecord): string {
    if (r.image_secs && !r.bench_tg_tps) return 'image';
    if (r.audio_secs && !r.bench_tg_tps) return 'asr';
    return 'llm';
  }
  function quantOf(name: string): string | null {
    return name.match(/(?:^|[-_. ])((?:IQ|Q)\d\w*|BF16|F16|F32)(?=$|[-_. ]|\.gguf)/i)?.[1] ?? null;
  }
  function scoresOf(r: ModelPerfRecord) {
    const file = library.models.find((m) => modelKeyOfFile(m.filename) === r.model_key);
    return scoreModel({
      task,
      fit: fits[r.model_key] ?? null,
      kind: kindOf(r),
      caps: { tools: undefined, vision: undefined, chat: true },
      paramsB: paramsFromName(r.model_name),
      quantization: file?.quantization || quantOf(r.model_name),
      measured: r,
    });
  }

  const fmtMs = (n: number | null) => (n === null ? '–' : t('ms.unit_ms', { n: Math.round(n) }));
  const fmtS = (n: number | null) => (n === null ? '–' : t('ms.unit_s', { n: Math.round(n * 10) / 10 }));
  const fmtTps = (n: number | null) => (n === null ? '–' : t('ms.unit_tps', { n: Math.round(n * 10) / 10 }));
  const fmtProbe = (r: ModelPerfRecord) => (r.tool_probe ? `${r.tool_probe.passed}/${r.tool_probe.total}` : '–');

  // Tool-call probe: against a running chat server.
  let servers = $derived(app.servers.filter((s) => s.status === 'Running' && s.active && !s.active.router_mode && (s.active.serve_mode ?? 'chat') === 'chat'));
  let serverId = $state('');
  $effect(() => { if (!servers.some((s) => s.id === serverId)) serverId = servers[0]?.id ?? ''; });
  let probing = $state(false);
  const serverLabel = (id: string) => {
    const s = app.servers.find((x) => x.id === id);
    return s?.active ? baseName(s.active.model_path) : id;
  };

  async function runProbe() {
    if (!serverId || probing) return;
    probing = true;
    try {
      const record = await invoke<ModelPerfRecord>('run_tool_probe', { serverId });
      await modelPerf.load(true);
      const p = record.tool_probe;
      if (p) toast.success(t('ms.probe_done', { passed: p.passed, total: p.total }));
    } catch (e) {
      toast.error(localizeError(e));
    } finally {
      probing = false;
    }
  }
</script>

<div class="flex h-full min-h-0 flex-col gap-4 overflow-y-auto p-6" data-testid="model-scores">
  <p class="max-w-3xl text-sm text-muted-foreground">{t('ms.page_hint')}</p>

  <section class="flex flex-col gap-2 rounded-lg border border-border p-4" aria-labelledby="ms-probe-title">
    <h2 id="ms-probe-title" class="text-sm font-semibold">{t('ms.probe_title')}</h2>
    <p class="max-w-3xl text-xs text-muted-foreground">{t('ms.probe_hint')}</p>
    {#if servers.length === 0}
      <p class="text-xs text-muted-foreground">{t('ms.probe_none')}</p>
    {:else}
      <div class="flex flex-wrap items-center gap-2">
        <label class="flex items-center gap-2 text-xs" for="ms-probe-server">
          <span>{t('ms.probe_server')}</span>
          <select id="ms-probe-server" class="h-8 rounded-md border border-input bg-background px-2 text-xs" bind:value={serverId} disabled={probing}>
            {#each servers as s (s.id)}<option value={s.id}>{serverLabel(s.id)}</option>{/each}
          </select>
        </label>
        <Button size="sm" variant="outline" onclick={runProbe} disabled={probing || !serverId}>
          {#if probing}<LoaderCircle class="animate-spin" />{:else}<ShieldCheck />{/if}{probing ? t('ms.probe_running') : t('ms.probe_run')}
        </Button>
      </div>
    {/if}
  </section>

  <div class="flex flex-wrap items-center gap-1" role="group" aria-label={t('ms.task_for')}>
    {#each TASKS as item (item.id)}
      <Button size="sm" variant={task === item.id ? 'secondary' : 'ghost'} aria-pressed={task === item.id} onclick={() => (task = item.id)}>{t(item.label)}</Button>
    {/each}
  </div>

  {#if modelPerf.failed && modelPerf.records.length === 0}
    <div class="flex items-center gap-3 text-xs text-destructive" role="alert">
      <span>{t('ms.read_failed')}</span>
      <Button size="xs" variant="outline" onclick={() => modelPerf.load(true)}>{t('perf.retry')}</Button>
    </div>
  {:else if modelPerf.records.length === 0}
    <div class="flex flex-col items-center gap-1 py-10 text-center">
      <p class="text-sm font-medium">{t('ms.empty')}</p>
      <p class="max-w-md text-xs text-muted-foreground">{t('ms.empty_hint')}</p>
    </div>
  {:else}
    <div class="min-w-0 overflow-x-auto rounded-lg border border-border">
      <Table.Root class="min-w-[760px]">
        <Table.Header>
          <Table.Row>
            <Table.Head>{t('ms.col_model')}</Table.Head>
            <Table.Head>{t('ms.col_device')}</Table.Head>
            <Table.Head class="text-right">{t('ms.col_load')}</Table.Head>
            <Table.Head class="text-right">{t('ms.col_generation')}</Table.Head>
            <Table.Head class="text-right">{t('ms.col_prompt')}</Table.Head>
            <Table.Head class="text-right">{t('ms.col_image')}</Table.Head>
            <Table.Head class="text-right">{t('ms.col_audio')}</Table.Head>
            <Table.Head class="text-right">{t('ms.col_tools')}</Table.Head>
          </Table.Row>
        </Table.Header>
        <Table.Body>
          {#each modelPerf.records as r (r.model_key + '|' + r.device)}
            <Table.Row data-model={r.model_key}>
              <Table.Cell class="min-w-56 max-w-[320px] whitespace-normal align-top">
                <div class="flex flex-col gap-2">
                  <span class="break-words text-sm font-medium [overflow-wrap:anywhere]">{r.model_name || r.model_key}</span>
                  <ScoreBreakdown compact scores={scoresOf(r)} />
                </div>
              </Table.Cell>
              <Table.Cell class="whitespace-normal align-top text-xs">{r.device}</Table.Cell>
              <Table.Cell class="text-right align-top font-num text-xs">{fmtMs(r.load_ms)}</Table.Cell>
              <Table.Cell class="text-right align-top font-num text-xs">{fmtTps(r.bench_tg_tps)}</Table.Cell>
              <Table.Cell class="text-right align-top font-num text-xs">{fmtTps(r.bench_pp_tps)}</Table.Cell>
              <Table.Cell class="text-right align-top font-num text-xs">{fmtS(r.image_secs)}</Table.Cell>
              <Table.Cell class="text-right align-top font-num text-xs">{fmtS(r.audio_secs)}</Table.Cell>
              <Table.Cell class="text-right align-top font-num text-xs">{fmtProbe(r)}</Table.Cell>
            </Table.Row>
          {/each}
        </Table.Body>
      </Table.Root>
    </div>
  {/if}
</div>
