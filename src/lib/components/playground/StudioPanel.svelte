<script lang="ts">
  // The Studios area: one switcher over task-specific workbenches. Every studio reuses an
  // existing Playground surface (image, speech, video, document) or hands off to chat and
  // the Agents module, so no screen is built twice. The strip under the switcher shows what
  // the studio calls and what it costs: API, streaming, engine, where it runs, memory.
  import { t } from '#lib/i18n.svelte.js';
  import { invoke } from '#lib/ipc.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { domain } from '#lib/stores/domain.svelte.js';
  import { library } from '#lib/stores/library.svelte.js';
  import { projects } from '#lib/stores/projects.svelte.js';
  import { formatBytes } from '#lib/format.js';
  import type { LibraryView } from '#lib/types.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import * as ToggleGroup from '#lib/components/ui/toggle-group/index.js';
  import ImageSurface from './ImageSurface.svelte';
  import TranscribePanel from './TranscribePanel.svelte';
  import TtsPanel from './TtsPanel.svelte';
  import AudioProcessPanel from './AudioProcessPanel.svelte';
  import OcrPanel from './OcrPanel.svelte';
  import VideoPanel from './VideoPanel.svelte';
  import { STUDIOS, STUDIO_STARTERS, studioById, studioResources, type StudioId } from './studios';
  import { isVideoTask } from './video';
  import { definitionFile } from './modelModes';
  import { AudioLines, Bot, ChartColumn, Code, FileText, Film, Image, Mic } from '@lucide/svelte';

  const ICONS = { image: Image, audio: AudioLines, video: Film, voice: Mic, document: FileText, coding: Code, analysis: ChartColumn, agent: Bot };

  let { studio = $bindable('image'), model, ocrModel, pipelineModel, pipelineTask }: {
    studio: StudioId;
    /** Model the header picker holds for this studio. */
    model: string;
    ocrModel: string;
    pipelineModel: string;
    pipelineTask?: string;
  } = $props();

  let spec = $derived(studioById(studio) ?? STUDIOS[0]);
  let frames = $state(9);
  let size = $state('256x256');
  let definition = $derived(domain.definitions.find(d => d.alias === model));
  /** The definition's VAE and text encoders, loaded with its model (sizes come from disk). */
  let companionBytes = $state(0);
  $effect(() => {
    const id = definition?.id;
    companionBytes = 0;
    if (!id) return;
    let live = true;
    invoke<number>('definition_companion_bytes', { id }).then((n) => { if (live) companionBytes = n; }).catch(() => {});
    return () => { live = false; };
  });
  let file = $derived(definition ? definitionFile(definition, library.models) : undefined);
  /** The engine family the model registry gives the file (MLX folders, mflux, mlx-vlm). */
  let runtime = $state<string | null>(null);
  $effect(() => {
    const path = file?.path;
    runtime = null;
    if (!path) return;
    let live = true;
    invoke<LibraryView>('list_library').then((view) => {
      if (live) runtime = view.entries.find(e => e.model.path === path)?.record.runtime ?? null;
    }).catch(() => {});
    return () => { live = false; };
  });
  let resources = $derived(studioResources(spec, model, file, { frames, size, companionBytes, runtime }));
  let chatStudio = $derived(spec.mode === 'chat');

  function openChat() {
    const starter = STUDIO_STARTERS[studio];
    if (starter) app.addToChat(starter);
    else app.openPlaygroundTab('chat');
  }
  function openAgents() {
    app.requestedModule = 'agents';
    app.openTab('modules');
  }
</script>

<div class="mx-auto flex w-full max-w-[960px] flex-col gap-3 px-4 pt-4">
  <ToggleGroup.Root
    type="single"
    variant="outline"
    size="sm"
    value={studio}
    onValueChange={(v) => { if (v) studio = v as StudioId; }}
    class="flex-wrap justify-start"
    aria-label={t('studio.switcher')}
  >
    {#each STUDIOS as s (s.id)}
      <ToggleGroup.Item value={s.id} class="h-7 px-2 text-xs">
        {@const Icon = ICONS[s.id]}<Icon class="size-3.5" aria-hidden="true" />{t(s.label)}
      </ToggleGroup.Item>
    {/each}
  </ToggleGroup.Root>
  <p class="m-0 text-sm text-muted-foreground">{t(spec.desc)}</p>
  <dl class="m-0 grid grid-cols-[repeat(auto-fit,minmax(150px,1fr))] gap-x-4 gap-y-2 rounded-md border border-border bg-muted/30 p-3 text-xs" aria-label={t('studio.resources')}>
    <div><dt class="text-muted-foreground">{t('studio.api')}</dt><dd class="m-0 font-mono break-all">{spec.api}</dd></div>
    <div><dt class="text-muted-foreground">{t('studio.streaming')}</dt><dd class="m-0">{spec.streaming ? t('studio.streaming_yes') : t('studio.streaming_no')}</dd></div>
    <div><dt class="text-muted-foreground">{t('studio.model')}</dt><dd class="m-0 break-all">{resources.alias || t('studio.no_model')}</dd></div>
    <div><dt class="text-muted-foreground">{t('studio.engine')}</dt><dd class="m-0">{resources.engine ?? t('studio.engine_unknown')}</dd></div>
    <div><dt class="text-muted-foreground">{t('studio.device')}</dt><dd class="m-0">{t(resources.location === 'local' ? 'setup.this_pc' : resources.location === 'remote' ? 'setup.remote' : 'setup.cloud')}</dd></div>
    <div>
      <dt class="text-muted-foreground">{t('studio.memory')}</dt>
      <dd class="m-0">{resources.memoryBytes ? t('studio.memory_value', { size: formatBytes(resources.memoryBytes, 1), file: formatBytes(resources.sizeBytes, 1) }) : t('studio.memory_unknown')}</dd>
    </div>
    <div><dt class="text-muted-foreground">{t('studio.outputs')}</dt><dd class="m-0">{projects.active?.folder ? t('studio.outputs_project', { project: projects.active.name }) : t('studio.outputs_none')}</dd></div>
  </dl>
</div>

{#if studio === 'image'}
  <ImageSurface />
{:else if studio === 'audio'}
  <TranscribePanel />
  <hr class="mx-auto w-full max-w-[860px] border-border" />
  <TtsPanel />
{:else if studio === 'voice'}
  <AudioProcessPanel />
{:else if studio === 'video'}
  {#if isVideoTask(pipelineTask)}
    {#key pipelineModel}<VideoPanel model={pipelineModel} task={pipelineTask} bind:frames bind:size />{/key}
  {:else}
    <p class="mx-auto w-full max-w-[960px] px-4 pt-4 text-sm text-muted-foreground" role="status">{t('studio.video_need_model')}</p>
  {/if}
{:else if studio === 'document'}
  <OcrPanel model={ocrModel} />
{:else if chatStudio}
  <div class="mx-auto flex w-full max-w-[960px] flex-wrap items-center gap-2 px-4 pt-4">
    <Button onclick={openChat}>{t(studio === 'agent' ? 'studio.open_chat' : 'studio.start_chat')}</Button>
    {#if studio === 'agent'}<Button variant="outline" onclick={openAgents}>{t('studio.open_agents')}</Button>{/if}
    <p class="m-0 basis-full text-xs text-muted-foreground">{t(studio === 'coding' ? 'studio.coding_hint' : studio === 'analysis' ? 'studio.analysis_hint' : 'studio.agent_hint')}</p>
  </div>
{/if}
