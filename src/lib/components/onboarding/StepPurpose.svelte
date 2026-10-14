<script lang="ts">
  import { PURPOSE_TOOLS } from './purposeCapabilities';
  import { getToolSettings, setToolSettings, type ToolSettings } from '#lib/tools-client.js';
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { Check, Info, LoaderCircle, MessageSquare, Code, FileSearch, Mic, ShieldAlert, TriangleAlert } from '@lucide/svelte';
  import type { Component } from 'svelte';
  import type { RuntimeDownloadProgress, RuntimeEntry, SystemHardwareInfo } from '#lib/types.js';
  import { formatBytes } from '#lib/format.js';
  import { localizeError } from '#lib/network.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { downloads } from '#lib/stores/downloads.svelte.js';
  import { library, normPath, repoRelative } from '#lib/stores/library.svelte.js';
  import { engineInstall } from '#lib/stores/engineInstall.svelte.js';
  import { runtimesStore } from '#lib/stores/runtimes.svelte.js';
  import { isPending } from '#lib/components/hub/downloadQueue.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Checkbox } from '#lib/components/ui/checkbox/index.js';
  import { Progress } from '#lib/components/ui/progress/index.js';
  import { cn } from '#lib/utils.js';
  import { PURPOSES, suggestFor, suggestionSelected, type ModelSuggestion, type Purpose, type PurposeResult, type Suggestion } from './suggest';
  import { findMcpServers, importMcpServers, MCP_SOURCE_LABEL, type FoundMcpServer } from './suggestMcp';

  interface Props {
    level: 'simple' | 'advanced';
    hardware: SystemHardwareInfo | null;
    /** Context length the fit estimate assumes (the Settings default). */
    ctx: number;
    /** True while an install runs; the wizard blocks navigation meanwhile. */
    busy?: boolean;
    /** Something was queued / installed here: the footer then says "Continue" instead of "Skip". */
    done?: boolean;
  }

  let { level = $bindable(), hardware, ctx, busy = $bindable(false), done = $bindable(false) }: Props = $props();

  const META: Record<Purpose, { icon: Component; label: TKey; desc: TKey }> = {
    chat: { icon: MessageSquare, label: 'onboarding.sg_purpose_chat', desc: 'onboarding.sg_purpose_chat_desc' },
    code: { icon: Code, label: 'onboarding.sg_purpose_code', desc: 'onboarding.sg_purpose_code_desc' },
    rag: { icon: FileSearch, label: 'onboarding.sg_purpose_rag', desc: 'onboarding.sg_purpose_rag_desc' },
    asr: { icon: Mic, label: 'onboarding.sg_purpose_asr', desc: 'onboarding.sg_purpose_asr_desc' },
  };

  const ROLE_LABEL: Record<string, TKey> = {
    chat: 'onboarding.sg_role_chat',
    code: 'onboarding.sg_role_code',
    embedding: 'onboarding.sg_role_embedding',
    reranker: 'onboarding.sg_role_reranker',
    asr: 'onboarding.sg_role_asr',
  };

  let picked = $state<Record<Purpose, boolean>>({ chat: false, code: false, rag: false, asr: false });
  let results = $state<Partial<Record<Purpose, PurposeResult | 'loading'>>>({});
  /** User overrides of the suggestions' initial selection. */
  let off = $state<Record<string, boolean>>({});
  /** Outcome per suggestion after "Install selected". */
  let outcome = $state<Record<string, { ok: boolean; text: string }>>({});

  let toolSettings = $state<ToolSettings | null>(null);
  let workspaceMessage = $state<string | null>(null);
  let workspaceBusy = $state(false);
  async function pickWorkspace() {
    if (busy || workspaceBusy || !toolSettings) return;
    workspaceBusy = true;
    busy = true;
    workspaceMessage = null;
    try {
      const workdir = await invoke<string | null>('pick_directory_dialog');
      if (workdir) {
        // Reload first: preserve policy, trusted tools and search settings.
        const fresh = await getToolSettings();
        toolSettings = await setToolSettings({ ...fresh, workdir });
        done = true;
      }
    } catch (e) { workspaceMessage = localizeError(e); }
    finally { workspaceBusy = false; busy = false; }
  }

  let mcpFound = $state<FoundMcpServer[]>([]);
  // Opt-in: a local (stdio) server runs programs with the user's rights, so nothing is pre-checked.
  let mcpOn = $state<Record<string, boolean>>({});
  let installError = $state<string | null>(null);
  let progress = $state<RuntimeDownloadProgress | null>(null);
  let timer: ReturnType<typeof setInterval> | null = null;
  let destroyed = false;

  onMount(() => {
    void getToolSettings().then((settings) => {
      if (!destroyed) toolSettings = settings;
    }).catch((e) => { if (!destroyed) workspaceMessage = localizeError(e); });
    void findMcpServers()
      .then((list) => {
        if (!destroyed) mcpFound = list;
      })
      .catch(() => {
        /* no readable import source: the section just stays hidden */
      });
  });
  onDestroy(() => {
    destroyed = true;
    if (timer) clearInterval(timer);
  });

  async function toggle(p: Purpose) {
    picked[p] = !picked[p];
    if (!picked[p] || results[p]) return;
    results[p] = 'loading';
    const r = await suggestFor(p, hardware, ctx);
    if (!destroyed) results[p] = r;
  }

  /** Already downloaded or in the queue: nothing to install. */
  function haveModel(s: ModelSuggestion): boolean {
    const job = downloads.jobFor(s.repoId, s.variant.name);
    if (job && (isPending(job.status) || job.status === 'Completed')) return true;
    // A folder model (MLX) is listed under the folder itself: `<library>/<owner>/<repo>`.
    const folder = `/${s.repoId.toLowerCase()}`;
    if (library.models.some((m) => normPath(m.path).endsWith(folder))) return true;
    const want = s.variant.name.toLowerCase();
    return library.forRepo(s.repoId).some((m) => {
      const rel = repoRelative(m, s.repoId);
      return rel !== null && rel.slice(rel.lastIndexOf('/') + 1).replace(/\.(gguf|bin)$/i, '').replace(/-\d{5}-of-\d{5}$/i, '').toLowerCase() === want;
    });
  }

  const shown = $derived(PURPOSES.filter((p) => picked[p]));
  const chosen = $derived(
    shown.flatMap((p) => {
      const r = results[p];
      if (!r || r === 'loading') return [];
      return r.suggestions.filter((s) => suggestionSelected(s, off[s.id]) && !outcome[s.id]?.ok && !(s.kind === 'model' && haveModel(s)));
    }),
  );
  const chosenMcp = $derived(mcpFound.filter((s) => mcpOn[s.key] && !outcome[s.key]?.ok));
  const totalBytes = $derived(chosen.reduce((n, s) => n + s.sizeBytes, 0));
  const actionCount = $derived(chosen.length + chosenMcp.length);
  const loading = $derived(shown.some((p) => results[p] === 'loading'));
  /** "Claude Code (3), Codex (2)": how many servers each source has. */
  const mcpSummary = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const s of mcpFound) counts.set(MCP_SOURCE_LABEL[s.source], (counts.get(MCP_SOURCE_LABEL[s.source]) ?? 0) + 1);
    return [...counts].map(([label, n]) => `${label} (${n})`).join(', ');
  });
  const pct = $derived(progress && progress.total_bytes > 0 ? Math.min(100, (progress.downloaded_bytes / progress.total_bytes) * 100) : 0);

  async function installRuntime(s: Extract<Suggestion, { kind: 'runtime' }>) {
    timer = setInterval(async () => {
      try {
        const p = await invoke<RuntimeDownloadProgress | null>('get_runtime_install_progress');
        if (!destroyed) progress = p;
      } catch {
        /* progress is best-effort */
      }
    }, 500);
    try {
      // The same call as Settings > Runtime > Add from GitHub.
      const entry = await invoke<RuntimeEntry>('install_runtime_github', {
        request: {
          family: s.pick.family,
          repo: s.pick.repo,
          tag: s.pick.tag,
          asset: s.pick.assets[0]?.name ?? null,
          backend: null,
          name: 'whisper.cpp',
          id: null,
          allow_unverified: false,
        },
      });
      outcome[s.id] = { ok: true, text: t('onboarding.sg_state_installed', { name: entry.name }) };
      await runtimesStore.load(true);
    } catch (e) {
      outcome[s.id] = { ok: false, text: localizeError(e) };
    } finally {
      if (timer) clearInterval(timer);
      timer = null;
      progress = null;
    }
  }

  async function install() {
    if (busy || actionCount === 0) return;
    busy = true;
    installError = null;
    try {
      // The runtime first: it is the one step that takes a while; model downloads only join the queue.
      for (const s of chosen) if (s.kind === 'runtime') await installRuntime(s);
      for (const s of chosen) {
        if (s.kind !== 'engine') continue;
        const ok = await engineInstall.install(s.family);
        outcome[s.id] = ok
          ? { ok: true, text: t('onboarding.sg_state_installed', { name: s.family }) }
          : { ok: false, text: engineInstall.state(s.family).error ?? t('onboarding.sg_engine_failed') };
      }
      for (const s of chosen) {
        if (s.kind !== 'model') continue;
        try {
          await downloads.enqueueVariant(s.repoId, s.variant);
          outcome[s.id] = { ok: true, text: t('onboarding.sg_state_queued') };
        } catch (e) {
          outcome[s.id] = { ok: false, text: localizeError(e) };
        }
      }
      if (chosenMcp.length > 0) {
        const list = [...chosenMcp];
        try {
          await importMcpServers(list);
          for (const s of list) outcome[s.key] = { ok: true, text: t('onboarding.sg_state_imported') };
        } catch (e) {
          installError = t('onboarding.sg_mcp_failed', { error: localizeError(e) });
        }
      }
      done = Object.values(outcome).some((o) => o.ok);
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex flex-col gap-5">
  <fieldset class="flex min-w-0 flex-col gap-2">
    <legend class="mb-2 text-sm font-semibold">{t('uiLevel.onboarding_title')}</legend>
    <div class="grid grid-cols-2 gap-2 max-[560px]:grid-cols-1">
      {#each ['simple', 'advanced'] as choice}
        <label class={cn('flex min-w-0 cursor-pointer flex-col gap-1 rounded-md border p-3 has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring', level === choice ? 'border-foreground/40 bg-muted' : 'border-border hover:bg-muted/50')}>
          <input type="radio" class="sr-only" name="onboarding-ui-level" value={choice} bind:group={level} disabled={busy} aria-describedby="ui-level-{choice}-description" />
          <span class="text-sm font-medium">{t(choice === 'simple' ? 'uiLevel.simple' : 'uiLevel.advanced')}{#if choice === 'simple'} <span class="text-xs text-muted-foreground">· {t('uiLevel.recommended')}</span>{/if}</span>
          <span id="ui-level-{choice}-description" class="text-xs leading-relaxed text-muted-foreground">{t(choice === 'simple' ? 'uiLevel.simple_desc' : 'uiLevel.advanced_desc')}</span>
        </label>
      {/each}
    </div>
  </fieldset>
  <div class="grid grid-cols-2 gap-2 max-[560px]:grid-cols-1" role="group" aria-label={t('onboarding.wizard_step_purpose_title')}>
    {#each PURPOSES as p (p)}
      {@const m = META[p]}
      <button
        type="button"
        aria-pressed={picked[p]}
        disabled={busy}
        class={cn(
          'flex min-w-0 items-start gap-2.5 rounded-lg border p-3 text-left transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-60',
          picked[p] ? 'border-primary/50 bg-primary/5' : 'border-border bg-card hover:bg-muted/50',
        )}
        onclick={() => toggle(p)}
      >
        <m.icon class={cn('mt-0.5 size-4 shrink-0', picked[p] ? 'text-primary' : 'text-muted-foreground')} />
        <span class="flex min-w-0 flex-1 flex-col gap-0.5">
          <span class="text-sm font-medium">{t(m.label)}</span>
          <span class="text-xs text-muted-foreground">{t(m.desc)}</span>
        </span>
        {#if picked[p]}<Check class="mt-0.5 size-4 shrink-0 text-primary" />{/if}
      </button>
    {/each}
  </div>

  {#each shown as p (p)}
    {@const r = results[p]}
    <section class="flex flex-col gap-2" aria-label={t(META[p].label)}>
      <h2 class="text-xs font-medium text-muted-foreground">{t(META[p].label)}</h2>
      {#if PURPOSE_TOOLS[p].length > 0}
        <div class="flex min-w-0 flex-col gap-2 rounded-lg border border-border bg-card p-3" data-purpose-tools={p}>
          <p class="text-sm font-medium">{t('onboarding.sg_tools_title')}</p>
          <p class="text-xs text-muted-foreground">{t(`onboarding.sg_tools_${p === 'asr' ? 'rag' : p}`)}</p>
          <div class="flex flex-wrap gap-1">{#each PURPOSE_TOOLS[p] as name}<code class="rounded bg-muted px-1.5 py-0.5 text-xs">{name}</code>{/each}</div>
          <p class="text-xs text-muted-foreground">{t('onboarding.sg_tools_available')}</p>
          {#if p === 'code' || p === 'rag'}
            <p class="break-all font-mono text-xs">{toolSettings?.workdir || t('onboarding.sg_workspace_empty')}</p>
            <Button variant="outline" size="sm" class="self-start" disabled={busy || workspaceBusy || !toolSettings} onclick={pickWorkspace}>{t('onboarding.sg_workspace_pick')}</Button>
            {#if workspaceMessage}<p class="text-xs text-destructive" role="alert">{workspaceMessage}</p>{/if}
          {/if}
        </div>
      {/if}
      {#if r === 'loading' || !r}
        <div class="flex items-center gap-2 rounded-lg border border-border bg-card p-3 text-xs text-muted-foreground" role="status">
          <LoaderCircle class="size-3.5 animate-spin" /> {t('onboarding.sg_loading')}
        </div>
      {:else}
        {#each r.suggestions as s (s.id)}
          {@const have = s.kind === 'model' && haveModel(s)}
          {@const out = outcome[s.id]}
          <label
            class={cn(
              'flex min-w-0 cursor-pointer items-start gap-2.5 rounded-lg border border-border bg-card p-3',
              (have || out?.ok) && 'cursor-default opacity-80',
            )}
          >
            <Checkbox
              class="mt-0.5"
              disabled={busy || have || out?.ok}
              checked={suggestionSelected(s, off[s.id]) && !have && !out?.ok}
              onCheckedChange={(v) => (off[s.id] = !v)}
            />
            <span class="flex min-w-0 flex-1 flex-col gap-1">
              <span class="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
                <span class="min-w-0 truncate text-sm font-medium" title={s.kind === 'model' ? s.variant.name : s.kind === 'engine' ? s.family : s.pick.tag}>
                  {s.kind === 'model' ? s.variant.name : s.kind === 'engine' ? s.family : `whisper.cpp ${s.pick.tag}`}
                </span>
                <Badge variant="secondary" class="text-xs">
                  {s.kind === 'model' ? t(ROLE_LABEL[s.role]) : t('onboarding.sg_role_runtime')}
                </Badge>
                <span class="font-num ml-auto text-xs text-muted-foreground">{formatBytes(s.sizeBytes, 1)}</span>
              </span>
              {#if s.kind === 'model'}
                <span class="truncate font-mono text-xs text-muted-foreground" title={s.repoId}>{s.repoId}</span>
              {/if}
              <span class="text-xs text-muted-foreground">{t(s.reason.key, s.reason.params)}</span>
              {#if s.kind === 'model' && s.toolEvidence}
                <span class={cn('text-xs', ['limited', 'unknown'].includes(s.toolEvidence) ? 'text-warning' : 'text-success')} data-code-tool-evidence={s.toolEvidence}>{t(`onboarding.sg_tools_evidence_${s.toolEvidence}`)}</span>
              {/if}
              {#if have}
                <span class="flex items-center gap-1 text-xs text-success"><Check class="size-3" /> {t('onboarding.sg_have')}</span>
              {:else if out}
                <span class={cn('flex items-start gap-1 text-xs break-words', out.ok ? 'text-success' : 'text-destructive')} role={out.ok ? 'status' : 'alert'}>
                  {#if out.ok}<Check class="mt-0.5 size-3 shrink-0" />{:else}<TriangleAlert class="mt-0.5 size-3 shrink-0" />{/if}
                  <span>{out.text}</span>
                </span>
              {/if}
            </span>
          </label>
        {/each}
        {#if r.suggestions.length === 0 && !r.error}
          <p class="text-xs text-muted-foreground">{t('onboarding.sg_none')}</p>
        {/if}
        {#if r.error}
          <p class="flex items-start gap-1.5 text-xs break-words text-destructive" role="alert">
            <TriangleAlert class="mt-0.5 size-3.5 shrink-0" /> <span>{t('onboarding.sg_failed', { error: localizeError(r.error) })}</span>
          </p>
        {/if}
        {#if r.note}
          <p class="flex items-start gap-1.5 text-xs break-words text-muted-foreground" role="status">
            <Info class="mt-0.5 size-3.5 shrink-0" /> <span>{localizeError(r.note)}</span>
          </p>
        {/if}
      {/if}
    </section>
  {/each}

  {#if mcpFound.length > 0}
    <section class="flex flex-col gap-2" aria-label={t('onboarding.sg_mcp_title')}>
      <h2 class="text-xs font-medium text-muted-foreground">{t('onboarding.sg_mcp_title')}</h2>
      <p class="text-sm">{t('onboarding.sg_mcp_found', { list: mcpSummary })}</p>
      <div class="flex max-h-56 flex-col gap-0.5 overflow-y-auto rounded-lg border border-border bg-card p-1">
        {#each mcpFound as s (s.key)}
          <label class="flex min-w-0 cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-xs hover:bg-muted/50">
            <Checkbox
              disabled={busy || outcome[s.key]?.ok}
              checked={!!mcpOn[s.key] && !outcome[s.key]?.ok}
              onCheckedChange={(v) => (mcpOn[s.key] = v)}
            />
            <span class="shrink-0 font-medium">{s.name}</span>
            <Badge variant="outline" class="shrink-0 text-muted-foreground">{MCP_SOURCE_LABEL[s.source]}</Badge>
            <code class="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground">
              {s.def.url ?? [s.def.command ?? '', ...(s.def.args ?? [])].join(' ')}
            </code>
            {#if outcome[s.key]?.ok}<Check class="size-3.5 shrink-0 text-success" />{/if}
          </label>
        {/each}
      </div>
      {#if mcpFound.some((s) => mcpOn[s.key] && s.def.command)}
        <p class="flex items-start gap-1.5 text-xs text-warning"><ShieldAlert class="mt-0.5 size-3 shrink-0" /> {t('conn.mcp_stdio_notice')}</p>
      {/if}
    </section>
  {/if}

  {#if installError}
    <p class="flex items-start gap-1.5 text-xs break-words text-destructive" role="alert">
      <TriangleAlert class="mt-0.5 size-3.5 shrink-0" /> <span>{installError}</span>
    </p>
  {/if}

  {#if progress}
    <div class="flex flex-col gap-1.5">
      <Progress value={pct} aria-label={t('onboarding.wizard_runtime_downloading')} />
      <span class="font-num text-xs text-muted-foreground">{formatBytes(progress.downloaded_bytes, 1)} / {formatBytes(progress.total_bytes, 1)}</span>
    </div>
  {/if}
  {#if engineInstall.busy('mlx-audio')}
    {@const pyPct = engineInstall.state('mlx-audio').pct}
    <div class="flex flex-col gap-1.5">
      <Progress value={pyPct ?? 0} aria-label={t('onboarding.wizard_runtime_downloading')} />
      <span class="text-xs text-muted-foreground">mlx-audio · {t('onboarding.wizard_runtime_downloading')}</span>
    </div>
  {/if}

  {#if shown.length > 0 || mcpFound.length > 0}
    <div class="flex flex-wrap items-center gap-x-3 gap-y-2">
      <Button onclick={install} disabled={busy || loading || actionCount === 0}>
        {#if busy}<LoaderCircle class="animate-spin" /> {t('onboarding.sg_installing')}{:else}{t('onboarding.sg_install_btn', { n: actionCount })}{/if}
      </Button>
      {#if totalBytes > 0}
        <span class="font-num text-xs text-muted-foreground">{t('onboarding.sg_total', { size: formatBytes(totalBytes, 1) })}</span>
      {/if}
    </div>
  {/if}
  {#if done}
    <p class="text-xs text-muted-foreground" role="status">{t('onboarding.sg_queued_note')}</p>
  {/if}
  <p class="text-xs text-muted-foreground">{t('onboarding.sg_later_note')}</p>
</div>
