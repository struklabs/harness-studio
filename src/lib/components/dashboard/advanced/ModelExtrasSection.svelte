<script lang="ts">
  import type { DraftSuggestion, LocalModel } from '$lib/types';
  import { invoke } from '$lib/ipc';
  import { t } from '$lib/i18n.svelte';
  import { Input } from '$lib/components/ui/input';
  import { capsStore } from '$lib/stores/caps.svelte';
  import type { LaunchParamsForm } from '../params';
  import Field from '../parts/Field.svelte';
  import NativeSelect from '../parts/NativeSelect.svelte';
  import AdvSection from './AdvSection.svelte';
  import CatalogFields from './CatalogFields.svelte';
  import FeatureBadge from './FeatureBadge.svelte';
  import MetadataOverrides from './MetadataOverrides.svelte';
  import ChatTemplateEditor from './ChatTemplateEditor.svelte';
  import { pathExample } from '$lib/platform';

  interface Props {
    page?: string;
    params: LaunchParamsForm;
    /** Candidates for the speculative-decoding draft model. */
    models: LocalModel[];
    selectedModelId: string;
  }

  let { page = 'other', params = $bindable(), models, selectedModelId }: Props = $props();

  const uid = $props.id();
  let selectedModel = $derived(models.find((m) => m.id === selectedModelId));
  let selectedPath = $derived(selectedModel?.path ?? '');
  /** `chat_template_file` lives in the catalog values (unset = removed). */
  function templateFile(): string {
    const v = params.catalog_params.chat_template_file;
    return typeof v === 'string' ? v : '';
  }
  function setTemplateFile(v: string) {
    if (v.trim()) params.catalog_params.chat_template_file = v;
    else delete params.catalog_params.chat_template_file;
  }
  let draftCandidates = $derived(
    models.filter((m) => m.id !== selectedModelId && m.is_ready && (!m.kind || m.kind === 'llm'))
  );
  /** Local drafts with the same tokenizer that are much smaller (`library::draft`). */
  let suggestedDrafts = $state<DraftSuggestion[]>([]);
  let otherDrafts = $derived(draftCandidates.filter((m) => !suggestedDrafts.some((s) => s.path === m.path)));
  $effect(() => {
    const path = selectedPath;
    suggestedDrafts = [];
    if (!path) return;
    let stale = false;
    invoke<DraftSuggestion[]>('suggest_draft_models', { modelPath: path })
      .then((list) => {
        if (!stale) suggestedDrafts = list;
      })
      .catch(() => {});
    return () => {
      stale = true;
    };
  });
</script>

<div hidden={page !== 'other'}><AdvSection open={true} title={t('dash.n_sec_speculative')}>
  {#if capsStore.supports('draft_model_path')}
    <div class="grid grid-cols-[minmax(0,1fr)_minmax(0,0.7fr)] gap-2">
      <Field
        label={t('dash.adv_draft_model_label')}
        id="{uid}-draft"
        help={t('help.draft')}
        hint={t('dash.adv_spec_hint')} note={suggestedDrafts.length > 0 && !params.draft_model_path ? t('def.draft_suggested_hint', { names: suggestedDrafts.map((s) => s.name).join(', ') }) : undefined}
        flag="-md / --spec-draft-model"
      >
        <NativeSelect id="{uid}-draft" bind:value={params.draft_model_path}>
          <option value="">{t('dash.adv_draft_model_none')}</option>
          {#if params.draft_model_path && !draftCandidates.some((m) => m.path === params.draft_model_path)}
            <option value={params.draft_model_path}>{params.draft_model_path}</option>
          {/if}
          {#if suggestedDrafts.length > 0}
            <optgroup label={t('def.draft_suggested')}>
              {#each suggestedDrafts as s (s.model_id)}
                <option value={s.path}>{s.name} ({s.quantization})</option>
              {/each}
            </optgroup>
            <optgroup label={t('def.draft_others')}>
              {#each otherDrafts as dm (dm.id)}
                <option value={dm.path}>{dm.name} ({dm.quantization})</option>
              {/each}
            </optgroup>
          {:else}
            {#each draftCandidates as dm (dm.id)}
              <option value={dm.path}>{dm.name} ({dm.quantization})</option>
            {/each}
          {/if}
        </NativeSelect>
      </Field>
      <Field help={t('help.draft_layers')} label={t('dash.adv_draft_ngl_label')} id="{uid}-dngl" flag="-ngld">
        <Input id="{uid}-dngl" type="number" min="0" placeholder={t('dash.adv_draft_ngl_placeholder')} bind:value={params.draft_gpu_layers} class="font-num" />
      </Field>
    </div>
  {/if}
  <CatalogFields bind:params group="speculative" />
</AdvSection>

</div>
<div hidden={page !== 'other'}>
<AdvSection open={true} title={t('dash.n_sec_model_files')}>
  {#if capsStore.supports('lora_path')}
    <div class="grid grid-cols-[minmax(0,1fr)_minmax(0,0.5fr)] gap-2">
      <Field help={t('help.lora')} label={t('dash.adv_lora_path_label')} id="{uid}-lora" flag="--lora">
        <Input id="{uid}-lora" type="text" placeholder={pathExample('C:\\models\\adapter.gguf', '~/models/adapter.gguf')} bind:value={params.lora_path} class="font-mono" />
      </Field>
      <Field help={t('help.lora_scale')} label={t('dash.adv_lora_scale_label')} id="{uid}-lorascale" flag="--lora-scaled">
        <Input id="{uid}-lorascale" type="number" step="0.05" min="0" max="2.0" placeholder="1.0" bind:value={params.lora_scale} class="font-num" />
      </Field>
    </div>
  {/if}
  {#if capsStore.supports('mmproj_path')}
    <Field label={t('dash.adv_mmproj_label')} id="{uid}-mmproj" hint={t('dash.adv_mmproj_hint')} flag="--mmproj">
      {#snippet aside()}<FeatureBadge feature="vision" />{/snippet}
      <Input id="{uid}-mmproj" type="text" placeholder={pathExample('C:\\models\\mmproj-model-f16.gguf', '~/models/mmproj-model-f16.gguf')} bind:value={params.mmproj_path} class="font-mono" />
    </Field>
  {/if}
  <CatalogFields bind:params group="model_files" exclude={['override_kv', 'chat_template_file']} />
  <Field label={t('param.override_kv')} help={t('param.override_kv_help')}>
    <MetadataOverrides bind:params {selectedModelId} />
  </Field>
</AdvSection>
</div>
<div hidden={page !== 'template'}>
  {#if capsStore.supports('chat_template')}
    <AdvSection open={true} title={t('dash.n_sec_chat_template')}>
    <Field help={t('help.chat_template')} label={t('dash.adv_chat_template_label')} id="{uid}-tpl" flag="--chat-template">
      <Input id="{uid}-tpl" type="text" placeholder={t('dash.adv_chat_template_placeholder')} bind:value={params.chat_template} />
    </Field>
    </AdvSection>
  {/if}
  {#if capsStore.supports('chat_template_file')}
    <AdvSection open={false} title={t('param.chat_template_file')}>
      <ChatTemplateEditor
        bind:file={templateFile, setTemplateFile}
        modelPath={selectedPath}
        jinjaOff={params.catalog_params.jinja === false}
        suggestedName={selectedModel?.name ?? ''}
      />
    </AdvSection>
  {/if}
</div>
