<script lang="ts">
  import type { LocalModel, ServeMode } from '#lib/types.js';
  import { t } from '#lib/i18n.svelte.js';
  import type { LaunchParamsForm } from '../params';
  import Field from '../parts/Field.svelte';
  import NativeSelect from '../parts/NativeSelect.svelte';
  import AdvSection from './AdvSection.svelte';
  import CatalogFields from './CatalogFields.svelte';

  interface Props {
    params: LaunchParamsForm;
    models: LocalModel[];
    selectedModelId: string;
  }

  let { params = $bindable(), models, selectedModelId }: Props = $props();

  const uid = $props.id();
  const MODES: ServeMode[] = ['chat', 'embedding', 'rerank'];
  const MODE_KEY = {
    chat: 'emb.mode_chat',
    embedding: 'emb.mode_embedding',
    rerank: 'emb.mode_rerank',
    transcription: 'emb.mode_transcription',
    image: 'emb.mode_image',
    pipeline: 'emb.mode_pipeline',
  } as const;

  let kind = $derived(models.find((m) => m.id === selectedModelId)?.kind);
  /** The mode "automatic" resolves to (same rule as `serve_mode::effective` in the backend). */
  let autoMode = $derived.by<ServeMode>(() => {
    const p = params.catalog_params;
    if (p.rerank === true) return 'rerank';
    if (p.embeddings === true) return 'embedding';
    if (kind === 'embedding') return 'embedding';
    if (kind === 'reranker') return 'rerank';
    return 'chat';
  });
  let chosen = $derived.by<ServeMode | ''>(() => {
    const v = params.catalog_params.serve_mode;
    return typeof v === 'string' && (MODES as string[]).includes(v) ? (v as ServeMode) : '';
  });
  let effective = $derived<ServeMode>(chosen || autoMode);
  let kindMode = $derived<ServeMode>(kind === 'embedding' ? 'embedding' : kind === 'reranker' ? 'rerank' : 'chat');

  let hint = $derived.by(() => {
    if (chosen && chosen !== kindMode && kind) return t('emb.mode_hint_forced');
    if (effective === 'embedding') return t('emb.mode_hint_embedding');
    if (effective === 'rerank') return t('emb.mode_hint_rerank');
    return undefined;
  });

  function setMode(v: string) {
    if (v) params.catalog_params.serve_mode = v;
    else delete params.catalog_params.serve_mode;
  }
</script>

<AdvSection open={true} title={t('caps.group.embeddings')} badge={effective !== 'chat' ? t(MODE_KEY[effective]) : undefined}>
  <Field label={t('emb.mode_label')} id="{uid}-mode" help={t('emb.mode_help')} note={hint}>
    <NativeSelect id="{uid}-mode" value={chosen} onchange={(e) => setMode(e.currentTarget.value)}>
      <option value="">{t('emb.mode_auto', { mode: t(MODE_KEY[autoMode]) })}</option>
      {#each MODES as m (m)}
        <option value={m}>{t(MODE_KEY[m])}</option>
      {/each}
    </NativeSelect>
  </Field>
  <CatalogFields bind:params group="embeddings" />
</AdvSection>
