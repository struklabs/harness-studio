<script lang="ts">
  import { Checkbox } from '#lib/components/ui/checkbox/index.js';
  import { readHideExternalSkills, saveHideExternalSkills } from './launchPreferences';
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  // "Başlat": starts a coding agent in a new terminal with process-only Harness settings (the
  // GUI side of `harness launch`). Through the gateway the tool may pick any definition (loaded
  // on first request); without it, the running server's model is used.
  import { invoke } from '#lib/ipc.js';
  import { rawError } from '#lib/errors.js';
  import { toast } from 'svelte-sonner';
  import { FolderOpen, MonitorUp, TriangleAlert, SlidersHorizontal } from '@lucide/svelte';
  import { Button } from '#lib/components/ui/button/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { t } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { domain } from '#lib/stores/domain.svelte.js';
  import type { ModelDefinition } from '#lib/types.js';
  import { cn } from '#lib/utils.js';
  import CopyButton from './CopyButton.svelte';
  import { launchCommand, launchTool, launchTools, opencodeDesktopApps, openOpencodeDesktop, type DesktopApp, type Integration, type LaunchToolId } from './integrations';
  import PrepareOptions from './PrepareOptions.svelte';
  import { kvText, type PreparePlan } from './agentTuning';
  import AgentModelNotes from './AgentModelNotes.svelte';
  import { findAgentModel, chatModelsOnly, opencodePromptTokens, type AgentModel } from './agentModels';

  interface Props {
    item: Integration | null;
    /** What the running server serves (direct mode). */
    served: { id: string; nCtx: number } | null;
    onClose: () => void;
    /** Direct mode: the existing "prepare the server" flow for the running server. */
    onTune: (item: Integration) => void;
    /** Chat models with their tool-calling verdict and prompt speed. */
    agentModels: AgentModel[];
    /** A tool check finished: reload `agentModels`. */
    onModelsChanged: () => void;
    /** OpenCode desktop: write the persistent config for `alias` (preview first), then open `appPath`. */
    onDesktop: (item: Integration, alias: string, appPath: string, hideExternalSkills: boolean) => void;
  }

  let { item, served, onClose, onTune, agentModels, onModelsChanged, onDesktop }: Props = $props();

  const RECOMMENDED: Record<LaunchToolId, number> = { claude: 65536, codex: 65536, opencode: 65536, aider: 32768, goose: 65536 };

  let alias = $state('');
  let hideExternalSkills = $state(readHideExternalSkills());
  let cwd = $state<string | null>(null);
  let busy = $state<'prepare' | 'launch' | null>(null);
  let error = $state('');
  let unsupported = $state(false);
  let preparePlan = $state<PreparePlan | null>(null);
  let selected = $state(-1);
  let plannedDefinition = $state('');
  let options = $derived(preparePlan ? [...(preparePlan.choice ? [preparePlan.choice] : []), ...preparePlan.alternatives] : []);
  let choice = $derived(selected >= 0 ? options[selected] : null);

  $effect(() => {
    const id = def?.id ?? '';
    const toolId = item?.launch;
    if (id !== plannedDefinition || !toolId) {
      preparePlan = null;
      selected = -1;
    }
  });

  let managed = $derived(app.gatewayRunning);
  let tool = $derived(item?.launch ?? null);
  // Offer only the shared chat catalog; wait for it instead of admitting speech/non-chat
  // definitions while metadata is loading.
  let definitions = $derived(
    managed ? domain.definitions.filter((d) => findAgentModel(chatModelsOnly(agentModels), d.alias) !== undefined) : []
  );
  let def = $derived<ModelDefinition | undefined>(definitions.find((d) => d.alias === alias));
  /** Path of the tool's executable (undefined = not looked up yet, null = not found). */
  let programPath = $state<string | null | undefined>(undefined);
  let desktops = $state<DesktopApp[]>([]);
  let desktop = $derived(desktops[0] ?? null);

  $effect(() => {
    if (!item) return;
    error = '';
    unsupported = false;
    preparePlan = null;
    selected = -1;
    programPath = undefined;
    desktops = [];
    if (managed) {
      void domain.refreshDefinitions();
      void domain.refreshStates();
    }
    const id = item.launch;
    void launchTools()
      .then((m) => (programPath = id ? (m[id] ?? null) : null))
      .catch(() => (programPath = undefined));
    if (id === 'opencode') {
      void opencodeDesktopApps()
        .then((a) => (desktops = a))
        .catch(() => (desktops = []));
    }
  });

  // Default model: the one the tool is already configured with, else a loaded one (the running
  // server's), else the first chat definition.
  $effect(() => {
    if (!item) return;
    if (managed) {
      if (!definitions.some((d) => d.alias === alias)) {
        const configured = definitions.find((d) => d.alias === item.configuredModel);
        alias =
          configured?.alias ??
          definitions.find((d) => domain.state(d.id)?.state === 'running')?.alias ??
          definitions[0]?.alias ??
          '';
      }
    } else {
      alias = served?.id ?? '';
    }
  });

  /** A desktop build's CLI stands in for a missing PATH `opencode`. */
  let bundledCli = $derived(desktop?.cli && programPath && programPath === desktop.cli ? desktop.cli : null);

  async function openDesktopOnly() {
    if (!desktop) return;
    try {
      await openOpencodeDesktop(desktop.path, hideExternalSkills);
      toast.success(t('agent.desktop_opened'));
      onClose();
    } catch (e) {
      const msg = rawError(e);
      error = localizeError(msg) || msg;
    }
  }

  function num(v: unknown): number | null {
    const n = typeof v === 'string' ? Number(v) : v;
    return typeof n === 'number' && Number.isFinite(n) && n > 0 ? n : null;
  }

  /** Context the tool will see: the definition's load setting (managed) or the live n_ctx. */
  let have = $derived(managed ? num(def?.load?.context_size) : served?.nCtx || null);
  let min = $derived(item?.minCtx ?? 0);
  let rec = $derived(tool ? RECOMMENDED[tool] : 0);
  let verdict = $derived<'ok' | 'small' | 'unknown'>(!min ? 'ok' : have === null ? 'unknown' : have >= min ? 'ok' : 'small');
  let endpointUrl = $derived(managed ? app.inferenceUrl : app.isRunning ? app.baseUrl : null);

  async function pickFolder() {
    try {
      const picked = await invoke<string | null>('pick_directory_dialog');
      if (picked) cwd = picked;
    } catch (e) {
      error = String(e);
    }
  }

  /** Preview the shared memory plan before changing the definition. */
  async function prepare() {
    if (!item || !tool || !def || busy) return;
    busy = 'prepare';
    error = '';
    const id = def.id;
    try {
      const plan = await invoke<PreparePlan>('plan_definition_prepare', { definitionId: id, toolId: tool, minCtx: min });
      if (def?.id !== id) return;
      plannedDefinition = id;
      preparePlan = plan;
      // Partial offload always needs an explicit selection.
      selected = plan.choice?.gpu_layers === null ? 0 : -1;
    } catch (e) {
      error = localizeError(rawError(e)) || rawError(e);
    } finally {
      busy = null;
    }
  }

  async function applyPrepare() {
    if (!item || !def || !choice || busy || def.id !== plannedDefinition) return;
    busy = 'prepare';
    error = '';
    try {
      const result = await invoke<{ error: string | null; restored: boolean; restore_error: string | null }>(
        'prepare_definition_for_agent', { definitionId: def.id, option: choice }
      );
      if (result.error) {
        error = t(result.restored ? 'intg.launch_prepare_restored' : result.restore_error ? 'intg.launch_prepare_restore_failed' : 'intg.launch_prepare_failed', {
          error: localizeError(result.error) || result.error,
          restoreError: result.restore_error ? (localizeError(result.restore_error) || result.restore_error) : '',
        });
      } else {
        preparePlan = null;
        selected = -1;
        toast.success(t('intg.launch_prepared', { alias: def.alias, name: item.name }));
      }
    } catch (e) {
      error = localizeError(rawError(e)) || rawError(e);
    } finally {
      await Promise.allSettled([domain.refreshDefinitions(), domain.refreshStates()]);
      busy = null;
    }
  }

  async function start() {
    if (!item || !tool || !alias || !endpointUrl || busy) return;
    busy = 'launch';
    error = '';
    try {
      const r = await launchTool({
        tool,
        alias,
        baseUrl: endpointUrl,
        apiKey: managed ? (app.gateway?.client_token ?? null) : (app.apiKey ?? null),
        contextSize: have,
        managed,
        cwd,
        hideExternalSkills,
      });
      toast.success(t('intg.launch_started', { name: item.name, pid: r.pid }));
      onClose();
    } catch (e) {
      const msg = rawError(e);
      if (msg.includes('launch.terminal_unsupported')) {
        unsupported = true;
      } else if (msg.includes('launch.not_installed')) {
        error = t('intg.launch_not_installed', { program: msg.split(':').pop()?.trim() ?? tool });
      } else {
        error = localizeError(msg) || msg;
      }
    } finally {
      busy = null;
    }
  }
</script>

<Dialog.Root
  bind:open={
    () => item !== null,
    (v) => {
      if (!v && !busy) onClose();
    }
  }
>
  <Dialog.Content class="max-h-[85vh] overflow-y-auto sm:max-w-lg">
    {#if item && tool}
      <Dialog.Header>
        <Dialog.Title>{t('intg.launch_title', { name: item.name })}</Dialog.Title>
        <Dialog.Description>{t('intg.launch_desc')}</Dialog.Description>
      </Dialog.Header>

      {#if !endpointUrl}
        <p class="rounded-md border border-warning/40 bg-warning/5 px-3 py-2 text-xs" role="alert">{t('intg.launch_no_endpoint')}</p>
      {:else}
        <div class="flex flex-col gap-3 text-xs">
          <p class="text-muted-foreground">
            {managed ? t('intg.launch_endpoint_managed', { url: endpointUrl }) : t('intg.launch_endpoint_direct', { url: endpointUrl })}
          </p>

          <label class="flex flex-col gap-1">
            <span class="font-medium">{t('intg.launch_model')}</span>
            {#if managed}
              <FieldHelp text={t('help.model_choice')}><select bind:value={alias} class="h-8 rounded-md border border-border bg-background px-2 text-xs" disabled={busy !== null}>
                {#each definitions as d (d.id)}
                  {@const st = domain.state(d.id)?.state}
                  {@const am = findAgentModel(agentModels, d.alias)}
                  <option value={d.alias}>
                    {d.alias}{st === 'running' ? ` · ${t('intg.model_loaded')}` : ''}{am?.tools === 'verified'
                      ? ` · ${t('agent.model_tools_ok')}`
                      : am?.tools === 'failed' || am?.tools === 'no_template'
                        ? ` · ${t('agent.model_tools_bad')}`
                        : ''}
                  </option>
                {/each}
              </select></FieldHelp>
            {:else}
              <span class="font-mono">{alias || '—'}</span>
            {/if}
          </label>

          {#if alias}
            <AgentModelNotes
              {alias}
              models={agentModels}
              toolName={item.name}
              promptTokens={tool === 'opencode' ? opencodePromptTokens(hideExternalSkills) : null}
              checkUrl={endpointUrl}
              checkKey={managed ? (app.gateway?.client_token ?? null) : (app.apiKey ?? null)}
              disabled={busy !== null}
              onPick={managed ? (a: string) => (alias = a) : null}
              onChecked={onModelsChanged}
            />
          {/if}

          {#if tool === 'opencode'}
            <label class="flex cursor-pointer items-start gap-2">
              <Checkbox checked={hideExternalSkills} onCheckedChange={(v) => { hideExternalSkills = v === true; saveHideExternalSkills(hideExternalSkills); }} disabled={busy !== null} />
              <span class="flex flex-col gap-1">
                <span>{t('agent.hide_external_skills')}</span>
                <span class="text-xs text-muted-foreground">{t('agent.hide_external_skills_tip')}</span>
              </span>
            </label>
          {/if}
          {#if tool === 'opencode' && desktop}
            <div class="flex flex-col gap-2 rounded-md border border-border px-3 py-2" role="note">
              <span class="break-words">
                {programPath === null ? t('agent.desktop_only', { path: desktop.path }) : t('agent.desktop_found', { path: desktop.path })}
              </span>
              {#if bundledCli}
                <span class="break-words text-muted-foreground">{t('agent.desktop_bundled_cli', { path: bundledCli })}</span>
              {/if}
              {#if desktop.running}
                <span class="flex items-start gap-1.5 text-muted-foreground">
                  <TriangleAlert class="mt-0.5 size-3.5 shrink-0 text-warning" />
                  <span>{t('agent.desktop_running')}</span>
                </span>
              {/if}
              <div class="flex flex-wrap gap-2">
                <Button
                  size="sm"
                  variant={programPath === null ? 'default' : 'outline'}
                  onclick={() => item && desktop && onDesktop(item, alias, desktop.path, hideExternalSkills)}
                  disabled={busy !== null || !alias}
                >
                  <MonitorUp />
                  {t('agent.desktop_configure_open')}
                </Button>
                {#if item.configured}
                  <Button size="sm" variant="ghost" onclick={openDesktopOnly} disabled={busy !== null}>{t('agent.desktop_open')}</Button>
                {/if}
              </div>
            </div>
          {:else if programPath === null}
            <p class="text-warning" role="alert">{t('intg.launch_not_installed', { program: tool })}</p>
          {/if}

          {#if min > 0}
            <div
              class={cn(
                'flex items-start gap-2 rounded-md border px-3 py-2',
                verdict === 'ok' ? 'border-border text-muted-foreground' : 'border-warning/40 bg-warning/5'
              )}
              role="status"
            >
              {#if verdict !== 'ok'}<TriangleAlert class="mt-0.5 size-3.5 shrink-0 text-warning" />{/if}
              <div class="flex min-w-0 flex-1 flex-col gap-2">
                <span>
                  {verdict === 'ok'
                    ? t('intg.launch_ctx_ok', { have: (have ?? 0).toLocaleString(), name: item.name })
                    : verdict === 'small'
                      ? t('intg.launch_ctx_small', { name: item.name, min: min.toLocaleString(), have: (have ?? 0).toLocaleString(), rec: rec.toLocaleString() })
                      : t('intg.launch_ctx_unknown', { name: item.name, min: min.toLocaleString() })}
                </span>
                {#if managed || verdict !== 'ok'}
                  {#if managed}
                    <Button size="sm" variant="outline" class="self-start" onclick={prepare} disabled={busy !== null || !def}>
                      <SlidersHorizontal />
                      {busy === 'prepare' ? t('intg.launch_preparing') : t('intg.launch_prepare')}
                    </Button>
                  {:else}
                    <Button size="sm" variant="outline" class="self-start" onclick={() => item && onTune(item)} disabled={busy !== null}>
                      <SlidersHorizontal />
                      {t('intg.launch_prepare')}
                    </Button>
                  {/if}
                {/if}
              </div>
            </div>
          {/if}

          {#if managed && preparePlan && def?.id === plannedDefinition}
            <div class="flex flex-col gap-2 rounded-md border border-border p-3">
              <PrepareOptions pp={preparePlan} {options} bind:selected name={item.name} disabled={busy !== null} />
              {#if choice}
                <AdvancedOnly>
                  <p class="text-muted-foreground">{t('intg.launch_prepare_desc', {
                    ctx: choice.context_size.toLocaleString(), kv: kvText(choice),
                    layers: choice.gpu_layers === null ? t('intg.launch_prepare_full_gpu') : t('intg.launch_prepare_layers', { n: choice.gpu_layers }),
                  })}</p>
                </AdvancedOnly>
              {/if}
              <Button size="sm" onclick={applyPrepare} disabled={busy !== null || !choice}>
                {busy === 'prepare' ? t('intg.launch_preparing') : t('intg.launch_prepare_apply')}
              </Button>
            </div>
          {/if}

          <div class="flex flex-col gap-1">
            <span class="font-medium">{t('intg.launch_folder')}</span>
            <div class="flex min-w-0 items-center gap-2">
              <span class="min-w-0 flex-1 truncate font-mono text-muted-foreground" title={cwd ?? ''}>{cwd ?? t('intg.launch_folder_default')}</span>
              <Button size="sm" variant="outline" onclick={pickFolder} disabled={busy !== null}>
                <FolderOpen />
                {t('intg.launch_folder_pick')}
              </Button>
            </div>
          </div>
        </div>
      {/if}

      {#if unsupported}
        <p class="text-xs text-warning" role="alert">{t('intg.launch_unsupported')}</p>
      {/if}
      {#if error}
        <p class="text-xs break-words text-destructive" role="alert">{error}</p>
      {/if}

      <div class="flex min-w-0 flex-col gap-1 text-xs">
        <span class="text-muted-foreground">{t('intg.launch_cli')}</span>
        <div class="flex min-w-0 items-center gap-2 rounded-md border border-border bg-muted px-2 py-1">
          <code class="min-w-0 flex-1 truncate font-mono">{launchCommand(tool, alias || '<model>', hideExternalSkills)}</code>
          <CopyButton text={launchCommand(tool, alias || '<model>', hideExternalSkills)} ariaLabel={t('intg.launch_cli')} variant="ghost" size="icon-sm" />
        </div>
      </div>

      <Dialog.Footer>
        <Button variant="outline" onclick={onClose} disabled={busy !== null}>{t('intg.launch_close')}</Button>
        <Button onclick={start} disabled={busy !== null || !alias || !endpointUrl || programPath === null}>
          {busy === 'launch' ? t('intg.launch_starting') : t('intg.launch_start')}
        </Button>
      </Dialog.Footer>
    {/if}
  </Dialog.Content>
</Dialog.Root>
