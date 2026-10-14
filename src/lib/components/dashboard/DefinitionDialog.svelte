<script lang="ts">
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import AdvancedOnly from '#lib/components/common/AdvancedOnly.svelte';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  // Create / edit a model definition: alias (the served model id), description, default
  // inference preset, LoRA adapters, projector, draft model and chat template. Saved at once.
  import { invoke } from '#lib/ipc.js';
  import { onMount } from 'svelte';
  import type { DraftSuggestion, LocalModel, ModelDefinition, ParamValues } from '#lib/types.js';
  import { presetName } from '#lib/components/presets/presetText.js';
  import { t } from '#lib/i18n.svelte.js';
  import { domain } from '#lib/stores/domain.svelte.js';
  import { capsStore } from '#lib/stores/caps.svelte.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import { Checkbox } from '#lib/components/ui/checkbox/index.js';
  import { Plus, X } from '@lucide/svelte';
  import Field from './parts/Field.svelte';
  import NativeSelect from './parts/NativeSelect.svelte';
  import CatalogFields from './advanced/CatalogFields.svelte';
  import ChatTemplateEditor from './advanced/ChatTemplateEditor.svelte';
  import RuntimePicker from './RuntimePicker.svelte';
  import type { LaunchParamsForm } from './params';
  import { definitionMode } from '../playground/modelModes';

  interface Props {
    /** The definition to edit, or a new one (empty id). */
    definition: ModelDefinition;
    models: LocalModel[];
    /** Put the cursor in the alias field (rename). */
    focusAlias?: boolean;
    onSaved: (def: ModelDefinition) => void;
    onClose: () => void;
  }

  let { definition, models, focusAlias = false, onSaved, onClose }: Props = $props();

  const uid = $props.id();
  const initial = () => structuredClone($state.snapshot(definition) as ModelDefinition);
  const d0 = initial();

  let alias = $state(d0.alias);
  let description = $state(d0.description ?? '');
  let presetId = $state(d0.default_preset_id ?? '');
  let mmproj = $state(d0.mmproj ?? '');
  let adapters = $state((d0.adapters ?? []).map((a) => ({ path: a.path, scale: a.scale ?? null })));
  let draftModel = $state(d0.draft?.model ?? '');
  /** Draft params as a catalog form (CatalogFields edits `catalog_params`). */
  let draftForm = $state({ catalog_params: { ...(d0.draft?.params ?? {}) } } as unknown as LaunchParamsForm);
  let draftNgl = $state<number | null>(typeof d0.draft?.params?.draft_gpu_layers === 'number' ? (d0.draft.params.draft_gpu_layers as number) : null);
  let chatTemplate = $state(d0.chat_template ?? '');
  let kwargs = $state(d0.chat_template_kwargs ?? '');
  /** Gateway: idle unload after N seconds (empty = gateway default) and pinned (warm). */
  let ttl = $state<number | null>(d0.ttl_seconds ?? null);
  let pinned = $state(!!d0.pinned);
  /** Analytics: store request/response content, kept N days (default 7). */
  let captureContent = $state(!!d0.capture_content);
  let contentDays = $state<number | null>(d0.content_retention_days ?? null);
  /** Runtime of this definition (`load.runtime`; '' = the official runtime). */
  let runtime = $state(typeof d0.load?.runtime === 'string' ? (d0.load.runtime as string) : '');
  /** Custom Jinja template file (`load.chat_template_file`, `--chat-template-file`). */
  let templateFile = $state(typeof d0.load?.chat_template_file === 'string' ? (d0.load.chat_template_file as string) : '');
  let saving = $state(false);
  let error = $state<string | null>(null);

  const isNew = !d0.id;
  const baseFile = d0.base.kind === 'local' ? d0.base.path : `hf:${d0.base.repo}:${d0.base.file}`;
  let chatFields = $derived(definitionMode(d0, models) === 'chat');

  const ALIAS_RE = /^[A-Za-z0-9._:-]{1,128}$/;
  let aliasError = $derived.by(() => {
    const a = alias.trim();
    if (!ALIAS_RE.test(a)) return t('def.alias_invalid');
    const clash = domain.definitions.find((d) => d.id !== d0.id && d.alias.toLowerCase() === a.toLowerCase());
    return clash ? t('def.alias_taken', { alias: clash.alias }) : null;
  });
  let kwargsError = $derived.by(() => {
    if (!chatFields) return null;
    if (!kwargs.trim()) return null;
    try {
      const v = JSON.parse(kwargs);
      return v && typeof v === 'object' && !Array.isArray(v) ? null : t('def.kwargs_object');
    } catch {
      return t('def.kwargs_object');
    }
  });

  let mmprojFiles = $derived(models.filter((m) => m.kind === 'mmproj'));
  let adapterFiles = $derived(models.filter((m) => m.kind === 'adapter'));
  let draftCandidates = $derived(
    models.filter((m) => m.is_ready && (!m.kind || m.kind === 'llm') && m.path !== baseFile)
  );
  /** Local drafts with the same tokenizer that are much smaller (My Models). */
  let suggestedDrafts = $state<DraftSuggestion[]>([]);
  let otherDrafts = $derived(draftCandidates.filter((m) => !suggestedDrafts.some((s) => s.path === m.path)));
  /** The projector that belongs to the model file (the library's pairing), while the field is empty. */
  let suggestedProjector = $state<{ path: string; name: string } | null>(null);
  let showProjectorSuggestion = $derived(!!suggestedProjector && !mmproj.trim());
  onMount(() => {
    if (d0.base.kind !== 'local') return;
    invoke<DraftSuggestion[]>('suggest_draft_models', { modelPath: d0.base.path })
      .then((list) => (suggestedDrafts = list))
      .catch(() => {});
    invoke<{ path: string; name: string } | null>('suggest_projector', { modelPath: d0.base.path })
      .then((found) => {
        if (!found) return;
        suggestedProjector = found;
        // A new definition starts with it; an existing one without a projector is only offered.
        if (isNew && !mmproj.trim()) mmproj = found.path;
      })
      .catch(() => {});
  });

  async function save(e: SubmitEvent) {
    e.preventDefault();
    if (saving || aliasError || kwargsError) return;
    saving = true;
    error = null;
    const draftParams: ParamValues = { ...(draftForm.catalog_params ?? {}) };
    delete draftParams.draft_model_path;
    if (draftNgl !== null && Number.isFinite(draftNgl)) draftParams.draft_gpu_layers = Math.trunc(draftNgl);
    else delete draftParams.draft_gpu_layers;
    const def: ModelDefinition = {
      ...d0,
      alias: alias.trim(),
      description: description.trim(),
      default_preset_id: presetId || null,
      mmproj: mmproj.trim() || null,
      adapters: adapters
        .filter((a) => a.path.trim())
        .map((a) => ({ path: a.path.trim(), scale: a.scale === null || !Number.isFinite(a.scale) ? null : a.scale })),
      draft: draftModel.trim() ? { model: draftModel.trim(), params: draftParams } : null,
      chat_template: chatTemplate.trim() || null,
      chat_template_kwargs: kwargs.trim() || null,
      load: (() => {
        const load: ParamValues = { ...(d0.load ?? {}) };
        if (runtime && runtime !== 'official') load.runtime = runtime;
        else delete load.runtime;
        if (templateFile.trim()) load.chat_template_file = templateFile.trim();
        else delete load.chat_template_file;
        return load;
      })(),
      ttl_seconds: ttl !== null && Number.isFinite(ttl) && ttl > 0 ? Math.trunc(ttl) : null,
      pinned,
      capture_content: captureContent,
      content_retention_days:
        captureContent && contentDays !== null && Number.isFinite(contentDays) && contentDays > 0
          ? Math.min(3650, Math.trunc(contentDays))
          : null,
    };
    try {
      const saved = await invoke<ModelDefinition>('save_model_definition', { definition: def });
      await domain.refreshDefinitions();
      onSaved(saved);
    } catch (err) {
      error = String(err);
    } finally {
      saving = false;
    }
  }

  function autofocus(node: HTMLElement) {
    if (focusAlias || isNew) queueMicrotask(() => node.focus());
  }
</script>

<Dialog.Root open={true} onOpenChange={(open) => !open && !saving && onClose()}>
  <Dialog.Content class="flex max-h-[88vh] flex-col sm:max-w-xl">
    <Dialog.Header>
      <Dialog.Title>{isNew ? t('def.dialog_new_title') : t('def.dialog_edit_title', { alias: d0.alias })}</Dialog.Title>
      <Dialog.Description class="truncate font-mono text-xs" title={baseFile}>{baseFile}</Dialog.Description>
    </Dialog.Header>

    <form onsubmit={save} class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto pr-1">
      <Field label={t('def.alias')} id="{uid}-alias" hint={t('def.alias_hint')} note={aliasError ?? undefined}>
        <input
          use:autofocus
          id="{uid}-alias"
          class="h-8 w-full min-w-0 rounded-lg border border-input bg-transparent px-2.5 font-mono text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 aria-invalid:border-destructive dark:bg-input/30"
          maxlength={128}
          bind:value={alias}
          aria-invalid={aliasError ? 'true' : undefined}
        />
      </Field>
      <Field help={t('help.description')} label={t('def.description')} id="{uid}-desc">
        <Input id="{uid}-desc" maxlength={500} bind:value={description} placeholder={t('def.description_placeholder')} />
      </Field>
      <Field label={t('def.default_preset')} id="{uid}-preset" hint={t('def.default_preset_hint')}>
        <div class="flex gap-1.5">
          <NativeSelect id="{uid}-preset" bind:value={presetId} class="flex-1">
            <option value="">{t('def.preset_none')}</option>
            {#each domain.presets as p (p.id)}
              <option value={p.id}>{presetName(p)}{p.builtin ? '' : ' ·'}</option>
            {/each}
          </NativeSelect>
          <Button type="button" variant="outline" size="sm" onclick={() => domain.openPresetManager(presetId || null)}>{t('ip.manage')}</Button>
        </div>
      </Field>

      {#if !uiLevel.advanced}
        <div class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
          {#if kwargsError}<span class="text-destructive" role="alert">{kwargsError}</span>{/if}
          <Button type="button" variant="ghost" size="xs" disabled={uiLevel.busy} onclick={() => uiLevel.set('advanced')}>{t('uiLevel.switch_advanced')}</Button>
        </div>
      {/if}
      <AdvancedOnly>
        <RuntimePicker bind:runtime modelPath={d0.base.kind === 'local' ? d0.base.path : null} compact hint={t('rt.def_hint')} />

        {#if chatFields}
        <div class="flex flex-col gap-2 rounded-md border border-border p-3">
          <span class="text-sm font-semibold text-foreground">{t('def.sec_files')}</span>
          {#if capsStore.supports('mmproj_path')}
            <Field label={t('dash.adv_mmproj_label')} id="{uid}-mmproj" hint={t('dash.adv_mmproj_hint')} flag="--mmproj">
              <Input id="{uid}-mmproj" list="{uid}-mmproj-list" class="font-mono text-xs" placeholder="mmproj-F16.gguf" bind:value={mmproj} />
              <datalist id="{uid}-mmproj-list">
                {#each mmprojFiles as m (m.id)}<option value={m.path}>{m.name}</option>{/each}
              </datalist>
              {#if showProjectorSuggestion && suggestedProjector}
                <div class="mt-1.5 flex flex-wrap items-center gap-2 text-xs text-muted-foreground" data-projector-suggestion>
                  <span class="min-w-0 truncate" title={suggestedProjector.path}>{t('def.projector_suggest', { name: suggestedProjector.name })}</span>
                  <Button type="button" variant="outline" size="xs" onclick={() => (mmproj = suggestedProjector?.path ?? '')}>{t('def.projector_apply')}</Button>
                </div>
              {/if}
            </Field>
          {/if}
          {#if capsStore.supports('lora_path')}
            <div class="flex flex-col gap-1.5">
              <span class="text-xs font-medium text-foreground/90">{t('def.adapters')}</span>
              {#each adapters as a, i (i)}
                <div class="grid grid-cols-[minmax(0,1fr)_72px_auto] gap-1.5">
                  <FieldHelp text={t('help.lora')}><Input list="{uid}-lora-list" class="font-mono text-xs" placeholder="adapter.gguf" bind:value={a.path} aria-label={t('def.adapter_path')} /></FieldHelp>
                  <FieldHelp text={t('help.lora_scale')}><Input type="number" step="0.05" min="0" max="4" class="font-num text-xs" placeholder="1.0" bind:value={a.scale} aria-label={t('def.adapter_scale')} /></FieldHelp>
                  <Button type="button" variant="ghost" size="icon-sm" aria-label={t('def.adapter_remove')} onclick={() => adapters.splice(i, 1)}><X /></Button>
                </div>
              {/each}
              <datalist id="{uid}-lora-list">
                {#each adapterFiles as m (m.id)}<option value={m.path}>{m.name}</option>{/each}
              </datalist>
              <Button type="button" variant="ghost" size="xs" class="self-start" onclick={() => adapters.push({ path: '', scale: null })}>
                <Plus />{t('def.adapter_add')}
              </Button>
            </div>
          {/if}
          {#if capsStore.supports('chat_template')}
            <div class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-2">
              <Field help={t('help.chat_template')} label={t('dash.adv_chat_template_label')} id="{uid}-tpl" flag="--chat-template">
                <Input id="{uid}-tpl" bind:value={chatTemplate} placeholder={t('dash.adv_chat_template_placeholder')} />
              </Field>
              <Field help={t('param.chat_template_kwargs_help')} label={t('def.kwargs')} id="{uid}-kw" note={kwargsError ?? undefined}>
                <Textarea id="{uid}-kw" rows={1} class="min-h-8 font-mono text-xs" placeholder={'{"enable_thinking": false}'} bind:value={kwargs} aria-invalid={kwargsError ? 'true' : undefined} />
              </Field>
            </div>
          {/if}
          {#if capsStore.supports('chat_template_file')}
            <ChatTemplateEditor bind:file={templateFile} modelPath={d0.base.kind === 'local' ? d0.base.path : ''} jinjaOff={d0.load?.jinja === false} suggestedName={alias.trim()} />
          {/if}
        </div>

        {#if capsStore.supports('draft_model_path')}
          <div class="flex flex-col gap-2 rounded-md border border-border p-3">
            <span class="text-sm font-semibold text-foreground">{t('dash.n_sec_speculative')}</span>
            <div class="grid grid-cols-[minmax(0,1fr)_minmax(0,0.6fr)] gap-2">
              <Field
                label={t('dash.adv_draft_model_label')}
                id="{uid}-draft"
                help={t('help.draft')}
                note={suggestedDrafts.length > 0 && !draftModel ? t('def.draft_suggested_hint', { names: suggestedDrafts.map((s) => s.name).join(', ') }) : undefined}
                flag="-md / --spec-draft-model"
              >
                <NativeSelect id="{uid}-draft" bind:value={draftModel}>
                  <option value="">{t('dash.adv_draft_model_none')}</option>
                  {#if draftModel && !draftCandidates.some((m) => m.path === draftModel)}
                    <option value={draftModel}>{draftModel}</option>
                  {/if}
                  {#if suggestedDrafts.length > 0}
                    <optgroup label={t('def.draft_suggested')}>
                      {#each suggestedDrafts as s (s.model_id)}
                        <option value={s.path}>{s.name} ({s.quantization})</option>
                      {/each}
                    </optgroup>
                    <optgroup label={t('def.draft_others')}>
                      {#each otherDrafts as m (m.id)}
                        <option value={m.path}>{m.name} ({m.quantization})</option>
                      {/each}
                    </optgroup>
                  {:else}
                    {#each draftCandidates as m (m.id)}
                      <option value={m.path}>{m.name} ({m.quantization})</option>
                    {/each}
                  {/if}
                </NativeSelect>
              </Field>
              <Field help={t('help.draft_layers')} label={t('dash.adv_draft_ngl_label')} id="{uid}-dngl" flag="-ngld">
                <Input id="{uid}-dngl" type="number" min="0" class="font-num" placeholder={t('dash.adv_draft_ngl_placeholder')} bind:value={draftNgl} disabled={!draftModel} />
              </Field>
            </div>
            {#if draftModel}
              <CatalogFields bind:params={draftForm} group="speculative" />
            {/if}
          </div>
        {/if}

        {/if}
        <div class="flex flex-col gap-2 rounded-md border border-border p-3">
          <span class="text-sm font-semibold text-foreground">{t('gw.def_section')}</span>
          <div class="grid grid-cols-[minmax(0,0.6fr)_minmax(0,1fr)] items-start gap-2">
            <Field label={t('gw.def_ttl')} id="{uid}-ttl" hint={t('gw.def_ttl_hint')}>
              <Input id="{uid}-ttl" type="number" min="1" class="font-num" placeholder="—" bind:value={ttl} />
            </Field>
            <Field label={t('gw.def_pinned')} id="{uid}-pinned" note={t('gw.def_pinned_hint')}>
              <Checkbox id="{uid}-pinned" bind:checked={pinned} aria-describedby="{uid}-pinned-help" />
            </Field>
          </div>
          <Field label={t('an.def_capture')} id="{uid}-capture" hint={t('an.def_capture_hint')} note={captureContent ? t('an.def_capture_warning') : undefined}>
            <Checkbox id="{uid}-capture" bind:checked={captureContent} />
          </Field>
          {#if captureContent}
            <Field label={t('an.def_capture_days')} id="{uid}-capture-days" hint={t('an.def_capture_days_hint')}>
              <Input id="{uid}-capture-days" type="number" min="1" max="3650" class="font-num" placeholder="7" bind:value={contentDays} />
            </Field>
          {/if}
        </div>

      </AdvancedOnly>
      {#if error}
        <p class="rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-2 text-xs text-destructive" role="alert">{error}</p>
      {/if}

      <Dialog.Footer class="sticky bottom-0 mx-0 mb-0 rounded-none bg-background px-0 pb-0 pt-2">
        <Button type="button" variant="outline" onclick={onClose} disabled={saving}>{t('ip.cancel')}</Button>
        <Button type="submit" disabled={saving || !!aliasError || !!kwargsError}>{saving ? t('ip.saving') : t('ip.save')}</Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
