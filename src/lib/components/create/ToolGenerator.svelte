<script lang="ts">
  import GenerationStatus from '#lib/components/create/GenerationStatus.svelte';
  import InfoTip from '#lib/components/common/InfoTip.svelte';
  import AttemptHistory from './AttemptHistory.svelte';
  import { collectProgress, emptyProgress } from './generatorProgress';
  import { tokenEstimate } from './client';
  import type { GeneratorEstimate } from '#lib/tools-client.js';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  // "Create a tool": the user describes a tool and the hosts and folders it
  // may reach; the chat model writes the package, the core tests it in the sandbox (retrying with
  // the errors up to its limit), and the user reads the code, the permissions and the test result
  // before approving. Approving registers the tool for this app session; "Save permanently"
  // writes it to <data dir>/tools/. Nothing here runs unless the user presses the button.
  import { onDestroy } from 'svelte';
  import { holdReleaseWhile } from '#lib/windowActivity.js';
  import { toast } from 'svelte-sonner';
  import { LoaderCircle, ShieldAlert, TriangleAlert } from '@lucide/svelte';
  import { t, locale, type TKey } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { rawError } from '#lib/errors.js';
  import GatewayError from '../common/GatewayError.svelte';
  import { cn } from '#lib/utils.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import ToolTestReport from '#lib/components/tools/ToolTestReport.svelte';
  import {
    approveGeneratedTool,
    cancelToolGeneration,
    discardToolDraft,
    generatedToolsSession,
    generateTool,
    modelParams,
    removeGeneratedTool,
    saveGeneratedTool,
    SMALL_MODEL_PARAMS,
    type GenerateToolRequest,
    type PermissionClass,
    type ToolDraft,
  } from '#lib/tools-client.js';

  interface Props {
    open: boolean;
    inline?: boolean;
    /** The chat model's endpoint; `null` while no model is available. */
    endpoint: GenerateToolRequest['endpoint'] | null;
    /** Display name of the chat model. */
    modelName: string;
    /** The GGUF behind it when it is a local server's own model (to size it up). */
    modelPath: string | null;
    /** A tool was approved or saved: the tool list changed. */
    onChanged: () => void;
    onUseModel?: (model: string) => void;
  }

  let { open = $bindable(), inline = false, endpoint, modelName, modelPath, onChanged, onUseModel }: Props = $props();

  type Phase = 'form' | 'running' | 'review' | 'approved';
  let phase = $state<Phase>('form');
  let description = $state('');
  let estimate = $state<GeneratorEstimate | null>(null);
  let estimating = $state(false);
  const fmt = (n: number) => Math.round(n).toLocaleString(locale());
  const range = (e: GeneratorEstimate) => e.high >= 10_000 ? `≈ ${(e.low / 1000).toLocaleString(locale(), {maximumFractionDigits:1})}–${(e.high / 1000).toLocaleString(locale(), {maximumFractionDigits:1})} ${t('create.token_thousand')} token` : `≈ ${fmt(e.low)}–${fmt(e.high)} token`;
  let hosts = $state('');
  let networkMode = $state<'none' | 'any' | 'listed'>('any');
  const networkScope = () => networkMode === 'any' ? ['*'] : networkMode === 'listed' ? lines(hosts) : [];
  let readDirs = $state('');
  let writeDirs = $state('');
  let draft = $state<ToolDraft | null>(null);
  let error = $state<string | null>(null);
  let progress = $state(emptyProgress());
  let saved = $state(false);
  let busy = $state(false);
  let session = $state<{ names: string[]; max: number }>({ names: [], max: 5 });
  let smallSize = $state<string | null>(null);
  // A generation in progress, a draft awaiting review or a typed description would be lost
  // with the page: keep it while hidden to the tray.
  $effect(() => holdReleaseWhile('tool-generator', () => phase === 'running' || phase === 'review' || (phase === 'form' && description.trim().length > 0)));

  const classLabel: Record<PermissionClass, TKey> = {
    safe: 'tools.class_safe',
    restricted: 'tools.class_restricted',
    dangerous: 'tools.class_dangerous',
  };
  const classTone: Record<PermissionClass, string> = {
    safe: 'text-success',
    restricted: 'text-info',
    dangerous: 'text-warning',
  };

  /** One entry per line (commas work too), trimmed, empty ones dropped, no duplicates. */
  const lines = (text: string) => [...new Set(text.split(/[\n,]/).map((s) => s.trim()).filter(Boolean))];

  let limitReached = $derived(session.names.length >= session.max);
  let canStart = $derived(!!endpoint && description.trim().length > 0 && !limitReached);
  $effect(() => {
    const selected = endpoint;
    const request = { description, network: networkScope(), fs_read: lines(readDirs), fs_write: lines(writeDirs) };
    let cancelled = false;
    // Keep the previous estimate (dimmed) until the new one arrives: the row is right-aligned,
    // so dropping it moved the start button under the cursor (S41).
    estimating = true;
    const timer = setTimeout(() => {
      if (selected && request.description.trim()) void tokenEstimate({ ...request, endpoint: selected })
        .then(value => { if (!cancelled) estimate = value; }).catch(() => { if (!cancelled) estimate = null; })
        .finally(() => { if (!cancelled) estimating = false; });
      else { estimate = null; estimating = false; }
    }, 400);
    return () => { cancelled = true; clearTimeout(timer); };
  });


  /** Takes a session tool out of the registry (a saved one stays under Kurulu araçlar). */
  async function removeSessionTool(name: string) {
    try {
      await removeGeneratedTool(name);
      await refreshSession();
      onChanged();
    } catch (e) {
      toast.error(t('tools.gen_failed', { error: localizeError(e) }));
    }
  }

  async function refreshSession() {
    try {
      const [names, max] = await generatedToolsSession();
      session = { names, max };
    } catch {
      // the count is a convenience; the core enforces the limit
    }
  }

  async function sizeUp() {
    smallSize = null;
    if (!modelPath) return;
    try {
      const n = await modelParams(modelPath);
      if (n && n < SMALL_MODEL_PARAMS) smallSize = n >= 1e9 ? `${(n / 1e9).toFixed(1)}B` : `${Math.round(n / 1e6)}M`;
    } catch {
      // no hint without a readable header
    }
  }

  // A fresh dialog starts at the form; the model and the session count are read each time.
  $effect(() => {
    if (!open) return;
    void refreshSession();
    void sizeUp();
  });

  async function start() {
    if (!endpoint || !canStart) return;
    error = null;
    phase = 'running';
    progress = emptyProgress();
    try {
      draft = await generateTool({
        description: description.trim(),
        network: networkScope(),
        fs_read: lines(readDirs),
        fs_write: lines(writeDirs),
        endpoint,
      }, value => { progress = collectProgress(progress, value); });
      phase = 'review';
    } catch (e) {
      error = rawError(e);
      phase = 'form';
    }
  }

  const stop = () => void cancelToolGeneration().catch(() => {});

  async function approve() {
    if (!draft || busy) return;
    busy = true;
    try {
      await approveGeneratedTool(draft.id);
      phase = 'approved';
      onChanged();
      void refreshSession();
    } catch (e) {
      toast.error(t('tools.gen_failed', { error: localizeError(e) }));
    } finally {
      busy = false;
    }
  }

  async function save() {
    if (!draft || busy) return;
    busy = true;
    try {
      await saveGeneratedTool(draft.info.name);
      saved = true;
      toast.success(t('tools.gen_saved', { name: draft.info.name }));
      onChanged();
      void refreshSession();
    } catch (e) {
      toast.error(t('tools.gen_failed', { error: localizeError(e) }));
    } finally {
      busy = false;
    }
  }

  /** Back to the form; a draft that was never approved is dropped from the core. */
  function again() {
    if (draft && phase === 'review') void discardToolDraft(draft.id).catch(() => {});
    draft = null;
    saved = false;
    error = null;
    phase = 'form';
  }

  function close() {
    if (phase === 'running') stop();
    // A saved tool is finished work: its description would otherwise keep the page from being
    // released in the tray (holdReleaseWhile above) for as long as the view stays mounted.
    if (phase === 'approved') description = '';
    if (draft && phase === 'review') void discardToolDraft(draft.id).catch(() => {});
    draft = null;
    saved = false;
    error = null;
    phase = 'form';
    open = false;
  }

  onDestroy(() => {
    if (phase === 'running') stop();
  });

  function permissionLines(d: ToolDraft): string[] {
    const i = d.info;
    const out = [
      i.network.includes('*') ? t('tools.network_any') : i.network.length ? t('tools.inst_perm_network', { hosts: i.network.join(', ') }) : t('tools.inst_perm_no_network'),
    ];
    if (i.fs_read.length) out.push(t('tools.inst_perm_read', { folders: i.fs_read.join(', ') }));
    if (i.fs_write.length) out.push(t('tools.inst_perm_write', { folders: i.fs_write.join(', ') }));
    if (!i.fs_read.length && !i.fs_write.length) out.push(t('tools.inst_perm_no_files'));
    out.push(t('tools.inst_perm_shell'));
    return out;
  }
  const existingHelpId = $props.id();
</script>

{#snippet content()}
    <div class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto pr-1">
      {#if phase === 'form' || phase === 'running'}
        {#if smallSize}
          <p class="flex items-start gap-1.5 text-xs text-warning" role="note">
            <TriangleAlert class="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
            {t('tools.gen_small', { model: modelName, size: smallSize })}
          </p>
        {/if}
        <label class="flex flex-col gap-1.5 text-xs font-medium">
          {t('tools.gen_what')}
          <FieldHelp text={t('help.tool_description')}><Textarea bind:value={description} rows={3} disabled={phase === 'running'} placeholder={t('tools.gen_what_ph')} class="resize-y text-sm font-normal" /></FieldHelp>
        </label>
        <label class="flex flex-col gap-1.5 text-xs font-medium">
          {t('tools.gen_network')}
          <select bind:value={networkMode} disabled={phase === 'running'} class="rounded-md border border-input bg-background p-2 text-sm">
            <option value="none">{t('tools.network_none')}</option>
            <option value="any">{t('tools.network_any')}</option>
            <option value="listed">{t('tools.network_listed')}</option>
          </select>
        </label>
        <div class="page-grid gap-3">
          <label class="flex flex-col gap-1.5 text-xs font-medium">
            {t('tools.gen_hosts')}
            <FieldHelp text={t('tools.gen_hosts_hint')}><Textarea bind:value={hosts} rows={3} disabled={phase === 'running' || networkMode !== 'listed'} placeholder={t('tools.gen_hosts_ph')} class="resize-y font-mono text-xs font-normal" /></FieldHelp>
            <span class="font-normal text-muted-foreground">{t('tools.gen_hosts_hint')}</span>
          </label>
          <label class="flex flex-col gap-1.5 text-xs font-medium">
            {t('tools.gen_read')}
            <Textarea aria-describedby={existingHelpId + '-existing-tools-gen_folders_hint'} bind:value={readDirs} rows={3} disabled={phase === 'running'} placeholder={t('tools.gen_folders_ph')} class="resize-y font-mono text-xs font-normal" />
          </label>
          <label class="flex flex-col gap-1.5 text-xs font-medium">
            {t('tools.gen_write')}
            <Textarea aria-describedby={existingHelpId + '-existing-tools-gen_folders_hint'} bind:value={writeDirs} rows={3} disabled={phase === 'running'} placeholder={t('tools.gen_folders_ph')} class="resize-y font-mono text-xs font-normal" />
          </label>
        </div>
        <p id={existingHelpId + '-existing-tools-gen_folders_hint'} data-field-help-text class="text-xs text-muted-foreground">{t('tools.gen_folders_hint')} {t('tools.gen_bound')}</p>
        <p class={cn('text-xs', limitReached ? 'text-warning' : 'text-muted-foreground')}>
          {limitReached ? t('tools.gen_limit', { max: session.max }) : t('tools.gen_session', { count: session.names.length, max: session.max })}
        </p>
        {#if session.names.length > 0}
          <ul class="flex flex-col gap-1" aria-label={t('tools.gen_session_list')}>
            {#each session.names as name (name)}
              <li class="flex items-center justify-between gap-2 rounded-md border border-border px-2 py-1">
                <span class="truncate font-mono text-xs">{name}</span>
                <Button variant="ghost" size="xs" aria-label={t('tools.gen_remove_aria', { name })} onclick={() => removeSessionTool(name)}>
                  {t('tools.gen_remove')}
                </Button>
              </li>
            {/each}
          </ul>
        {/if}
        {#if !endpoint}
          <p class="text-xs text-warning">{t('tools.gen_no_model')}</p>
        {/if}
        {#if error}
          <GatewayError {error} onUse={onUseModel} onRetry={start} />
          <p data-manual-tokens class="font-num text-xs">{t('create.tokens_spent')}: {progress.usage.estimated ? '≈ ' : ''}<span class="inline-block min-w-[12ch] whitespace-nowrap">{fmt(progress.usage.prompt_tokens + progress.usage.completion_tokens)}</span> <InfoTip text={t('create.token_usage_help', { input: fmt(progress.usage.prompt_tokens), output: fmt(progress.usage.completion_tokens) })} /></p>
          <AttemptHistory history={progress.history} />
        {/if}
      {:else if draft}
        <div class="flex flex-col gap-1">
          <p class="text-sm font-medium">{t('tools.gen_review', { name: draft.info.name, version: draft.info.version })}</p>
          <p class="text-xs text-muted-foreground">{draft.info.description}</p>
          <p class="text-xs text-muted-foreground">{draft.attempts === 1 ? t('tools.gen_attempts_one') : t('tools.gen_attempts', { n: draft.attempts })}</p>
        </div>

        <div class="flex flex-col gap-1.5 rounded-md border p-3">
          <p class="text-xs font-medium">{t('tools.inst_confirm_asks')}</p>
          <ul class="list-disc pl-5 text-xs">
            {#each permissionLines(draft) as line (line)}
              <li>{line}</li>
            {/each}
          </ul>
          <Badge variant="outline" class={cn('mt-1 w-fit gap-1', classTone[draft.info.class])}>
            {#if draft.info.class === 'dangerous'}<ShieldAlert />{/if}
            {t(classLabel[draft.info.class])}
          </Badge>
        </div>

        <div class="flex flex-col gap-1.5 rounded-md border p-3">
          <p data-manual-tokens class="font-num text-xs">{t('create.tokens_spent')}: {draft.usage?.estimated ? '≈ ' : ''}<span class="inline-block min-w-[12ch] whitespace-nowrap">{fmt((draft.usage?.prompt_tokens ?? 0) + (draft.usage?.completion_tokens ?? 0))}</span> <InfoTip text={t('create.token_usage_help', { input: fmt(draft.usage?.prompt_tokens ?? 0), output: fmt(draft.usage?.completion_tokens ?? 0) })} /></p>
          <AttemptHistory history={draft.history ?? []} />
          <ToolTestReport report={draft.report} />
          {#if !draft.report.passed}
            <p class="text-xs text-destructive" role="alert">{draft.suspect ? t('tools.gen_test_conflict', { case: draft.suspect.case, actual: draft.suspect.actual.slice(0, 120), expected: draft.suspect.expected.slice(0, 120) }) : t('tools.gen_test_failed')}</p>
          {/if}
        </div>

        {#each [['tools.gen_code', draft.code], ['tools.gen_test_code', draft.test], ['tools.gen_manifest', draft.manifest_json]] as [label, text] (label)}
          <div class="flex min-w-0 flex-col gap-1">
            <span class="text-xs font-medium">{t(label as TKey)}</span>
            <pre class={cn('max-h-56 overflow-auto rounded-md border bg-muted p-2.5 font-mono text-xs leading-snug whitespace-pre', draft.suspect && label === 'tools.gen_test_code' && 'border-warning ring-2 ring-warning/50')}>{text}</pre>
          </div>
        {/each}

        {#if phase === 'approved'}
          <p class="text-xs text-success" role="status">
            {t(saved ? 'tools.gen_saved_note' : 'tools.gen_approved', { name: draft.info.name })}
          </p>
        {/if}
      {/if}
    </div>

    <div class="flex flex-wrap items-center justify-end gap-2">
      {#if phase === 'running'}
        <span class="mr-auto flex items-center gap-1.5 text-xs text-muted-foreground" role="status">
          <LoaderCircle class="size-3.5 animate-spin" aria-hidden="true" />
          <GenerationStatus {progress} />
        </span>
        <Button variant="outline" onclick={stop}>{t('tools.gen_stop')}</Button>
      {:else if phase === 'form'}
        {#if !inline}<Button variant="outline" onclick={close}>{t('tools.gen_close')}</Button>{/if}
        <Button disabled={!canStart} onclick={start}>{t('tools.gen_start')}</Button>
        {#if estimate}<span data-manual-token-estimate class="text-xs text-muted-foreground transition-opacity" class:opacity-50={estimating}>{range(estimate)} <InfoTip text={t('create.token_manual_estimate_help', { prompt: fmt(estimate.prompt_per_tool), reply: fmt(estimate.reply_per_tool) })} /></span>{/if}
      {:else if phase === 'review'}
        <Button variant="outline" onclick={again}>{t('tools.gen_again')}</Button>
        <Button disabled={busy || !draft?.report.passed} onclick={approve}>{t('tools.gen_approve')}</Button>
      {:else}
        {#if !inline}<Button variant="outline" onclick={close}>{t('tools.gen_close')}</Button>{/if}
        {#if !saved}
          <Button disabled={busy} onclick={save}>{t('tools.gen_save')}</Button>
        {/if}
      {/if}
    </div>
{/snippet}
{#if inline}
  <section class="flex max-w-4xl flex-col gap-4">
    <p class="text-xs text-muted-foreground">{t('tools.gen_desc')}</p>
    {@render content()}
  </section>
{:else}
  <Dialog.Root bind:open onOpenChange={(o) => !o && close()}>
    <Dialog.Content class="flex max-h-[90vh] flex-col gap-3 sm:max-w-2xl">
      <Dialog.Header><Dialog.Title>{t('tools.gen_title')}</Dialog.Title><Dialog.Description>{t('tools.gen_desc')}</Dialog.Description></Dialog.Header>
      {@render content()}
    </Dialog.Content>
  </Dialog.Root>
{/if}
