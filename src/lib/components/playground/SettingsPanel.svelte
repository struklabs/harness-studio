<script lang="ts">
  import Field from '../dashboard/parts/Field.svelte';
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { visibleGroups, pageLevel } from '#lib/uiLevel.js';
  import AdvancedPageNotice from '../common/AdvancedPageNotice.svelte';
  import { PLAYGROUND_GROUPS } from '../common/screenSections';
  import { SectionSelection } from '#lib/stores/sectionPage.svelte.js';
  import RagSettings from './RagSettings.svelte';
  import type { RagModels } from './rag';
  const section = new SectionSelection('playground', PLAYGROUND_GROUPS.flatMap(g => g.pages.map(p => p.id)), undefined, undefined, PLAYGROUND_GROUPS);
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  import { modelLabel } from '#lib/format.js';
  import { presetName, presetDescription } from '#lib/components/presets/presetText.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { cn } from '#lib/utils.js';
  import { presetModified, type PgSettings, type ToolsState } from './types';
  import type { ModelLoadState } from './gatewayModels';
  import { domain } from '#lib/stores/domain.svelte.js';
  import RequestParamsEditor from '../presets/RequestParamsEditor.svelte';
  import CompactionPanel from './CompactionPanel.svelte';
  import NativeSelect from '../dashboard/parts/NativeSelect.svelte';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { groupTools } from '#lib/mcp-client.js';
  import type { ServerTool } from '#lib/mcp-types.js';
  import { security } from '#lib/stores/security.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import { Switch } from '#lib/components/ui/switch/index.js';
  import { Checkbox } from '#lib/components/ui/checkbox/index.js';
  import * as UiField from '#lib/components/ui/field/index.js';
  import * as Select from '#lib/components/ui/select/index.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import { Plus, RefreshCw, RotateCcw, ShieldAlert, Settings2 } from '@lucide/svelte';

  interface Props {
    ragModels?: RagModels | null;
    onEmbedChange?: () => void;
    onGoToHub?: () => void;
    settings: PgSettings;
    models: string[];
    /** Gateway: alias -> load state (empty without the gateway). */
    modelStates?: Record<string, ModelLoadState>;
    modelsLoading: boolean;
    modelsError: boolean;
    disabled: boolean;
    onRefreshModels: () => void;
    onReset: () => void;
    tools: ServerTool[];
    toolsState: ToolsState;
    toolsError: string;
    onRefreshTools: () => void;
    /** Opens the tool generator dialog (the user starts it; never automatic). */
    onCreateTool: () => void;
    /** Applies a preset (also "reset to preset"). */
    onApplyPreset: (id: string) => void;
    /** Saves the current settings as a new user preset. */
    onSaveAsPreset: () => void;
    /** The chat model's context in tokens (compaction threshold preview). */
    contextSize?: number | null;
  }

  let {
    ragModels = null,
    onEmbedChange = () => {},
    onGoToHub = () => {},
    settings = $bindable(),
    models,
    modelStates = {},
    modelsLoading,
    modelsError,
    disabled,
    onRefreshModels,
    onReset,
    tools,
    toolsState,
    toolsError,
    onRefreshTools,
    onCreateTool,
    onApplyPreset,
    onSaveAsPreset,
    contextSize = null,
  }: Props = $props();

  let preset = $derived(domain.preset(settings.presetId));
  let modified = $derived(presetModified(settings, preset));

  const uid = $props.id();
  const id = (name: string) => `pg-${name}-${uid}`;

  // Harness tools first, then one group per MCP server (Harness's own MCP client; only the ticked
  // tools' schemas go to the model).
  let groups = $derived.by(() => {
    const out: { group: string; tools: ServerTool[] }[] = [];
    const own = tools.filter((tl) => tl.type === 'harness');
    if (own.length) out.push({ group: '', tools: own });
    out.push(...groupTools(tools.filter((tl) => tl.type === 'mcp')));
    return out;
  });

  /** A tool that can change the machine (write files, run commands): marked with the shield. */
  const needsCare = (tool: ServerTool) => tool.class === 'dangerous';

  let activeCount = $derived(tools.filter((tl) => !settings.disabledTools.includes(tl.tool)).length);

  function setToolEnabled(name: string, on: boolean) {
    const rest = settings.disabledTools.filter((n) => n !== name);
    settings.disabledTools = on ? rest : [...rest, name];
  }

  function groupOn(g: { tools: ServerTool[] }) {
    return g.tools.every((tl) => !settings.disabledTools.includes(tl.tool));
  }

  function setGroup(g: { tools: ServerTool[] }, on: boolean) {
    const names = new Set(g.tools.map((tl) => tl.tool));
    const rest = settings.disabledTools.filter((n) => !names.has(n));
    settings.disabledTools = on ? rest : [...rest, ...names];
  }

  function setAll(on: boolean) {
    settings.disabledTools = on ? [] : tools.map((tl) => tl.tool);
  }

</script>

<aside class="flex h-full min-h-0 flex-col bg-card" aria-labelledby={id('title')}>
  <div class="flex h-11 shrink-0 items-center justify-between border-b border-border pr-2 pl-4">
    <h2 id={id('title')} class="text-sm font-semibold text-foreground">{t('pg.settings_title')}</h2>
    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="ghost" size="icon-sm" aria-label={t('pg.reset_settings')} onclick={onReset}>
            <RotateCcw />
          </Button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content>{t('pg.reset_settings')}</Tooltip.Content>
    </Tooltip.Root>
  </div>

  <nav class="shrink-0 border-b border-border p-3" aria-label={t('pg.settings_title')} data-panel-sections="playground">
    <NativeSelect title={t('pg.settings_title')} value={section.active} aria-label={t('pg.settings_title')} onchange={e => section.select(e.currentTarget.value)}>
      {#if pageLevel(PLAYGROUND_GROUPS, section.active) === 'advanced' && !uiLevel.advanced}
        <option value={section.active} hidden>{t(PLAYGROUND_GROUPS.flatMap(g => g.pages).find(p => p.id === section.active)!.label)}</option>
      {/if}
      {#each visibleGroups(PLAYGROUND_GROUPS, uiLevel.level) as group}
        <optgroup label={t(group.label!)}>
          {#each group.pages as page}<option value={page.id}>{t(page.label)}</option>{/each}
        </optgroup>
      {/each}
    </NativeSelect>
  </nav>
  <!-- One native scroll surface fills the panel; the tool list must not nest another. -->
  <div data-settings-scroll class="min-h-0 flex-1 overflow-y-auto overscroll-contain">
    <AdvancedPageNotice level={pageLevel(PLAYGROUND_GROUPS, section.active)} />
    <h3 class="px-4 pt-4 text-sm font-semibold">{t(PLAYGROUND_GROUPS.flatMap(g => g.pages).find(p => p.id === section.active)!.label)}</h3>
    <UiField.Group class="gap-5 p-4">
<div hidden={section.active !== 'model'} class="flex flex-col gap-3">
      <!-- Model -->
      <Field label={t('pg.model')} id={id('model')} hint={t('pg.model_hint')}>
        <div class="flex min-w-0 gap-1.5">
          <Select.Root type="single" bind:value={settings.model} disabled={disabled || models.length === 0}>
            <Select.Trigger id={id('model')} class="min-w-0 flex-1 font-mono text-xs" aria-invalid={modelsError}>
              <span class="truncate">
                {settings.model ? modelLabel(settings.model) : modelsError ? t('pg.model_error') : t('pg.model_none')}
              </span>
            </Select.Trigger>
            <!-- The panel sits at the window's right edge: grow leftwards and never past the viewport. -->
            <Select.Content
              align="end"
              collisionPadding={8}
              class="max-w-[min(28rem,var(--bits-select-content-available-width))] min-w-(--bits-select-anchor-width)"
            >
              {#each models as m (m)}
                <Select.Item
                  value={m}
                  label={modelLabel(m)}
                  class="font-mono text-xs *:[span]:last:min-w-0 *:[span]:last:shrink"
                  title={m}
                >
                  <span class="min-w-0 flex-1 truncate">{modelLabel(m)}</span>
                  {#if modelStates[m]}
                    <span class={cn('shrink-0 font-sans text-xs', modelStates[m] === 'loaded' ? 'text-success' : 'text-muted-foreground')}>
                      {t(modelStates[m] === 'loaded' ? 'intg.model_loaded' : modelStates[m] === 'loading' ? 'intg.model_loading' : 'intg.model_unloaded')}
                    </span>
                  {/if}
                </Select.Item>
              {/each}
            </Select.Content>
          </Select.Root>
          <Tooltip.Root>
            <Tooltip.Trigger>
              {#snippet child({ props })}
                <Button
                  {...props}
                  variant="outline"
                  size="icon"
                  aria-label={t('pg.model_refresh')}
                  onclick={onRefreshModels}
                  disabled={disabled || modelsLoading}
                >
                  <RefreshCw class={cn(modelsLoading && 'animate-spin')} />
                </Button>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content>{t('pg.model_refresh')}</Tooltip.Content>
          </Tooltip.Root>
        </div>
        {#if modelsError}
          <UiField.Error>{t('pg.model_error')}</UiField.Error>
        {/if}
      </Field>

      <!-- Inference preset -->
      <Field label={t('ip.preset')} id={id('preset')} hint={t('ip.preset_hint')} note={preset?.builtin && preset.id !== 'builtin:default' ? t(('ip.note_' + preset.id.slice(8)) as TKey) : undefined} help={preset && !preset.builtin ? presetDescription(preset) : undefined}>
        <div class="flex items-center justify-end gap-2">
          <span class="flex items-center gap-1">
            {#if modified}
              <Badge variant="outline" class="h-5 border-warning/40 text-xs text-warning">{t('ip.modified')}</Badge>
              <Tooltip.Root>
                <Tooltip.Trigger>
                  {#snippet child({ props })}
                    <Button {...props} variant="ghost" size="icon-xs" aria-label={t('ip.reset_to_preset')} onclick={() => onApplyPreset(settings.presetId)}>
                      <RotateCcw />
                    </Button>
                  {/snippet}
                </Tooltip.Trigger>
                <Tooltip.Content>{t('ip.reset_to_preset')}</Tooltip.Content>
              </Tooltip.Root>
            {/if}
          </span>
        </div>
        <div class="flex min-w-0 gap-1.5">
          <NativeSelect
            id={id('preset')}
            class="min-w-0 flex-1 text-xs"
            value={settings.presetId}
            onchange={(e: Event) => onApplyPreset((e.currentTarget as HTMLSelectElement).value)}
          >
            {#if !preset}<option value={settings.presetId}>{settings.presetId ? t('ip.missing_preset') : t('ip.no_preset')}</option>{/if}
            <optgroup label={t('ip.builtins')}>
              {#each domain.presets.filter((p) => p.builtin) as p (p.id)}
                <option value={p.id}>{presetName(p)}</option>
              {/each}
            </optgroup>
            {#if domain.presets.some((p) => !p.builtin)}
              <optgroup label={t('ip.mine')}>
                {#each domain.presets.filter((p) => !p.builtin) as p (p.id)}
                  <option value={p.id}>{presetName(p)}</option>
                {/each}
              </optgroup>
            {/if}
          </NativeSelect>
          <Tooltip.Root>
            <Tooltip.Trigger>
              {#snippet child({ props })}
                <Button {...props} variant="outline" size="icon" aria-label={t('ip.manage')} onclick={() => domain.openPresetManager(settings.presetId || null)}>
                  <Settings2 />
                </Button>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content>{t('ip.manage')}</Tooltip.Content>
          </Tooltip.Root>
        </div>
        {#if preset?.builtin && preset.description}
          <AdvancedOnly><UiField.Description class="text-xs">{presetDescription(preset)}</UiField.Description></AdvancedOnly>
        {/if}
        {#if modified}
          <Button variant="link" size="xs" class="h-auto self-start p-0 text-xs" onclick={onSaveAsPreset}>{t('ip.save_as_new')}</Button>
        {/if}
      </Field>


</div>
<div hidden={section.active !== 'system'} class="flex flex-col gap-3">
      <!-- System prompt -->
      <Field label={t('pg.system_prompt')} id={id('system')} hint={t('help.system_prompt')}>
        <Textarea
          id={id('system')}
          rows={3}
          class="min-h-16 resize-y text-xs"
          placeholder={t('pg.system_prompt_placeholder')}
          bind:value={settings.systemPrompt}
        />
      </Field>

</div>
<div hidden={section.active !== 'model'} class="flex flex-col gap-3">
      <Field advanced label={t('pg.auto_title')} id={id('autotitle')} hint={t('pg.auto_title_hint')}>
        <Switch id={id('autotitle')} bind:checked={settings.autoTitle} size="sm" />
      </Field>
</div>
<div hidden={!['sampling', 'output'].includes(section.active)} class="flex flex-col gap-3">
      <!-- Request settings (preset) -->
      <UiField.Set class="gap-3">
        <RequestParamsEditor bind:request={settings.request} compact page={section.active === 'output' ? 'output' : 'sampling'} />
      </UiField.Set>


</div>
<div hidden={section.active !== 'tools'} class="flex flex-col gap-3">
      <!-- Tools -->
      <UiField.Set class="gap-3">
        <Field label={t('pg.tools_enable')} id={id('tools')} hint={t('pg.tools_hint')}>
          <Switch id={id('tools')} checked={settings.toolsEnabled} onCheckedChange={(v) => { settings.toolsEnabled = v; settings.toolsExplicit = true; }} size="sm" />
        </Field>

        {#if settings.toolsEnabled}
          <div class="flex items-center justify-between gap-2">
            <span
              class={cn('min-w-0 text-xs leading-snug', toolsState === 'error' ? 'text-destructive' : 'text-muted-foreground')}
              role="status"
              aria-live="polite"
            >
              {#if toolsState === 'loading'}
                {t('pg.tools_loading')}
              {:else if toolsState === 'ok'}
                {t('pg.tools_count', { active: activeCount, total: tools.length })}
              {:else if toolsState === 'disabled'}
                {t('pg.tools_disabled')}
              {:else if toolsState === 'error'}
                {t('pg.tools_error', { message: toolsError })}
              {:else}
                {t('pg.tools_offline')}
              {/if}
            </span>
            <Tooltip.Root>
              <Tooltip.Trigger>
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon-sm"
                    aria-label={t('pg.tools_refresh')}
                    onclick={onRefreshTools}
                    disabled={disabled || toolsState === 'loading'}
                  >
                    <RefreshCw class={cn(toolsState === 'loading' && 'animate-spin')} />
                  </Button>
                {/snippet}
              </Tooltip.Trigger>
              <Tooltip.Content>{t('pg.tools_refresh')}</Tooltip.Content>
            </Tooltip.Root>
          </div>

          <Button variant="link" size="xs" class="h-auto justify-start p-0 text-xs" onclick={() => security.openSettings('tools')}>
            {t('pg.tools_permissions_link')}
          </Button>

          <Tooltip.Root>
            <Tooltip.Trigger>
              {#snippet child({ props })}
                <Button {...props} variant="outline" size="sm" class="w-fit" onclick={onCreateTool} {disabled}>
                  <Plus />
                  {t('tools.gen_open')}
                </Button>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content class="max-w-64">{t('tools.gen_open_hint')}</Tooltip.Content>
          </Tooltip.Root>

          {#if toolsState === 'ok' && tools.length > 0}
            <div class="flex gap-3">
              <Button variant="link" size="xs" class="h-auto p-0 text-xs" onclick={() => setAll(true)}>{t('pg.tools_select_all')}</Button>
              <Button variant="link" size="xs" class="h-auto p-0 text-xs" onclick={() => setAll(false)}>{t('pg.tools_select_none')}</Button>
            </div>
            <div data-tool-list class="space-y-3 rounded-md border border-border p-2.5">
              {#each groups as g (g.group)}
                <div class="flex min-w-0 flex-col gap-1.5">
                  <div class="flex min-w-0 items-center gap-2">
                    <FieldHelp text={t('help.tool_group')}><Checkbox
                      id={id(`group-${g.group || 'harness'}`)}
                      checked={groupOn(g)}
                      onCheckedChange={(c) => setGroup(g, c === true)}
                      aria-label={t('pg.tools_group_toggle')}
                    /></FieldHelp>
                    <label for={id(`group-${g.group || 'harness'}`)} class="min-w-0 flex-1 cursor-pointer truncate text-xs font-medium text-muted-foreground">
                      {g.group === '' ? t('pg.tools_group_harness') : t('pg.tools_group_mcp', { name: g.group })}
                    </label>
                  </div>
                  {#each g.tools as tool (tool.tool)}
                    {@const desc = tool.definition.function.description ?? ''}
                    <div class="flex min-w-0 items-center gap-2">
                      <FieldHelp text={t('help.tool_enabled')}><Checkbox
                        id={id(`tool-${tool.tool}`)}
                        checked={!settings.disabledTools.includes(tool.tool)}
                        onCheckedChange={(c) => setToolEnabled(tool.tool, c === true)}
                      /></FieldHelp>
                      <Tooltip.Root>
                        <Tooltip.Trigger class="min-w-0 flex-1 text-left">
                          {#snippet child({ props })}
                            <label {...props} for={id(`tool-${tool.tool}`)} class="block cursor-pointer truncate font-mono text-xs text-foreground">
                              {tool.tool}
                            </label>
                          {/snippet}
                        </Tooltip.Trigger>
                        {#if desc}
                          <Tooltip.Content class="max-w-64">{desc}</Tooltip.Content>
                        {/if}
                      </Tooltip.Root>
                      {#if needsCare(tool)}
                        <Tooltip.Root>
                          <Tooltip.Trigger>
                            {#snippet child({ props })}
                              <span {...props} class="shrink-0 text-warning" aria-label={t('pg.tool_needs_approval')}>
                                <ShieldAlert class="size-3.5" />
                              </span>
                            {/snippet}
                          </Tooltip.Trigger>
                          <Tooltip.Content class="max-w-64">{t('pg.tool_dangerous_title')}</Tooltip.Content>
                        </Tooltip.Root>
                      {/if}
                    </div>
                  {/each}
                </div>
              {/each}
            </div>
          {/if}
        {/if}
      </UiField.Set>

</div>
<div hidden={section.active !== 'history'} class="flex flex-col gap-3">
      <!-- Context compaction -->
      <CompactionPanel bind:settings {models} {contextSize} />
</div>
      <div hidden={section.active !== 'rag'}>
        <RagSettings bind:settings models={ragModels} {onEmbedChange} {onGoToHub} />
      </div>
    </UiField.Group>
  </div>
</aside>
