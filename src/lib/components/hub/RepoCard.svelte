<script lang="ts">
  import { app } from '#lib/stores/app.svelte.js';
  import { engineInstall } from '#lib/stores/engineInstall.svelte.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import HelpPopover from '#lib/components/common/HelpPopover.svelte';
  import { t, locale, type TKey } from '#lib/i18n.svelte.js';
  import { onnxUnsupportedText } from '#lib/engines.js';
  import type { HfModelSearchResult, ModelCapability } from '#lib/types.js';
  import { cn } from '#lib/utils.js';
  import { library } from '#lib/stores/library.svelte.js';
  import PublisherAvatar from './PublisherAvatar.svelte';
  import UpdateBadge from './UpdateBadge.svelte';
  import { showUpdateBadge } from '#lib/downloadFollowUp.js';
  import { isDecisionRepo, taskInfo } from './tasks';
  import { formatName, notRunnableKey, taskSourceKey } from './inference';
  import { taskSupportStore } from './taskSupport.svelte';
  import { canConvert, speechNeedsConversion } from './convertible';
  import { CircleCheck, Download, Heart, Lock, Play, Ban, Info, Repeat } from '@lucide/svelte';

  interface Props {
    repo: HfModelSearchResult;
    selected: boolean;
    layout: 'grid' | 'list';
    /** Quant currently used as a client-side filter (highlighted in the chips). */
    quantFilter: string;
    onSelect: (repoId: string) => void;
  }

  let { repo, selected, layout, quantFilter, onSelect }: Props = $props();

  let license = $derived.by(() => {
    if (repo.license) return repo.license;
    const l = repo.tags?.find((x) => x.toLowerCase().startsWith('license:'));
    return l ? l.slice('license:'.length) : null;
  });
  // Capabilities come from the registry record (task + format). Tool support is not shown: the Hub
  // listing carries only a cut chat template, so it would be a guess from the name.
  /** The Hub chips of the MLX engines, by family. */
  const MLX_CHIPS: Record<string, { label: TKey; title: TKey; needs: TKey; needsTitle: TKey }> = {
    mlx: { label: 'hub.compat_mlx', title: 'hub.compat_mlx_title', needs: 'hub.compat_needs_mlx', needsTitle: 'hub.compat_needs_mlx_title' },
    'mlx-vlm': { label: 'hub.compat_mlx_vlm', title: 'hub.compat_mlx_vlm_title', needs: 'hub.compat_needs_mlx_vlm', needsTitle: 'hub.compat_needs_mlx_vlm_title' },
    'mlx-audio': { label: 'hub.compat_mlx_audio', title: 'hub.compat_mlx_audio_title', needs: 'hub.compat_needs_mlx_audio', needsTitle: 'hub.compat_needs_mlx_audio_title' },
  };
  /** mlx-audio runs speech recognition too: its chip then says it transcribes. */
  const MLX_STT_CHIP = { label: 'hub.compat_mlx_audio_stt', title: 'hub.compat_mlx_audio_stt_title', needs: 'hub.compat_needs_mlx_audio', needsTitle: 'hub.compat_needs_mlx_audio_title' } as const;
  const mlxChip = (runtime: string) =>
    runtime === 'mlx-audio' && (repo.pipeline_tag ?? repo.inferred_task) === 'automatic-speech-recognition' ? MLX_STT_CHIP : MLX_CHIPS[runtime];

  const CAP_KEY: Record<ModelCapability, TKey> = {
    chat: 'hub.cap_chat',
    vision: 'hub.cap_vision',
    audio: 'hub.cap_audio',
    embedding: 'hub.cap_embedding',
    rerank: 'hub.cap_rerank',
    asr: 'hub.cap_asr',
    tts: 'hub.cap_tts',
    image_generation: 'hub.cap_image',
  };
  let capLabels = $derived((repo.capabilities ?? []).map((c) => t(CAP_KEY[c])));
  let owner = $derived(repo.id.includes('/') ? repo.id.split('/')[0] : (repo.author ?? ''));
  let name = $derived(repo.id.includes('/') ? repo.id.slice(repo.id.indexOf('/') + 1) : repo.id);
  let quants = $derived(repo.quants ?? []);
  let localModels = $derived(library.forRepo(repo.id));
  let downloaded = $derived(localModels.length > 0);
  let running = $derived(localModels.some((m) => library.isRunning(m)));
  let hasUpdate = $derived(showUpdateBadge(library.updates, repo.id, downloaded));
  let recommended = $derived(library.recommended[repo.id] ?? null);
  /**
   * Only warn when the repo's task is known: many GGUF re-uploads carry no pipeline_tag, so the
   * backend infers one from tags / files (`inferred_task`); the card says so.
   */
  /** A decision model's pipeline_tag (text-classification...) does not say how Harness runs it. */
  let decision = $derived(isDecisionRepo(repo));
  let task = $derived(taskInfo(decision ? 'decision-model' : (repo.pipeline_tag ?? repo.inferred_task)));
  // A decision repo's task comes from its tag, not from an inference.
  let inferred = $derived(!decision && !repo.pipeline_tag && !!repo.inferred_task);
  let support = $derived(taskSupportStore.support(task?.id));
  /** Family that would run the task but is not installed (e.g. stable-diffusion.cpp for images). */
  let taskNeeds = $derived(
    // A diffusers pipeline runs on the Python engine, not on the task's usual stable-diffusion.cpp.
    'runtime' in repo.compatibility && repo.compatibility.runtime === 'python-hf'
      ? null
      : (taskSupportStore.info(task?.id)?.needs_runtime ?? null)
  );
  /** Runtimes that run a repo of a task whose usual family is another one (CrispASR GGUFs, MLX
   *  speech): their verdict stands and the task's "needs whisper.cpp" hint does not apply. */
  const OWN_RUNTIMES = ['audio.cpp', 'crispasr', 'mlx', 'mlx-vlm', 'mlx-audio'];
  let compat = $derived(repo.compatibility);
  /** A safetensors chat model: not run as it is, but the drawer offers the GGUF conversion. */
  let convertible = $derived(canConvert(repo));
  /** No Harness runtime runs this format: the card is dimmed, the drawer has no download. */
  let unsupported = $derived((compat.status === 'unsupported' && !convertible) || compat.status === 'unsupported_platform');

  function compact(n: number): string {
    return new Intl.NumberFormat(locale(), { notation: 'compact', maximumFractionDigits: 1 }).format(n);
  }
</script>

<!-- The help icon sits inside the card's top-right corner, over the card button (a button cannot nest
     inside another), instead of beside the card where it broke the grid's alignment. -->
<div class="relative h-full min-w-0">
<button
  type="button"
  class={cn(
    'group flex h-full w-full min-w-0 flex-col gap-2 rounded-lg border bg-card p-3 pr-10 text-left transition-colors focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none',
    selected ? 'border-primary/60 bg-primary/5' : 'border-border hover:border-foreground/25 hover:bg-muted/40',
    layout === 'list' && 'sm:flex-row sm:items-center sm:gap-4',
    unsupported && 'opacity-60'
  )}
  aria-pressed={selected}
  aria-label={t('hub.open_repo_aria', { repo: repo.id })}
  onclick={() => onSelect(repo.id)}
>
  <div class={cn('flex min-w-0 items-center gap-2.5', layout === 'list' && 'sm:w-[34%] sm:shrink-0')}>
    <PublisherAvatar name={owner} />
    <div class="flex min-w-0 flex-col gap-0.5">
      <div class="flex min-w-0 items-center gap-1.5">
        <span class="truncate text-sm font-medium" title={repo.id}>{name}</span>
        {#if repo.gated}
          <Lock class="size-3 shrink-0 text-warning" aria-hidden="true" />
        {/if}
      </div>
      <span class="truncate text-xs text-muted-foreground">{owner}</span>
    </div>
  </div>

  <div class="flex shrink-0 items-center gap-3 font-num text-xs text-muted-foreground">
    <!-- Some repos (ggerganov/whisper.cpp) are not download-tracked: HF reports 0, so show nothing. -->
    {#if repo.downloads > 0}
      <span class="flex items-center gap-1" title={t('hub.downloads_count', { count: repo.downloads.toLocaleString(locale()) })}>
        <Download class="size-3" />{compact(repo.downloads)}
      </span>
    {/if}
    <span class="flex items-center gap-1" title={t('hub.likes_count', { count: repo.likes.toLocaleString(locale()) })}>
      <Heart class="size-3" />{compact(repo.likes)}
    </span>
    {#if repo.last_modified}
      <span>{new Date(repo.last_modified).toLocaleDateString(locale())}</span>
    {/if}
  </div>

  <!-- State as coloured text, capabilities as one line; licence and details are in the drawer. -->
  <div class="flex min-w-0 flex-1 flex-wrap items-center gap-x-2.5 gap-y-0.5 text-xs">
    {#if running}
      <span class="inline-flex items-center gap-1 text-success"><Play class="size-3" />{t('hub.badge_running')}</span>
    {:else if downloaded}
      <span class="inline-flex items-center gap-1 text-success"><CircleCheck class="size-3" />{t('hub.badge_downloaded')}</span>
    {/if}
    {#if convertible}
      <span class="inline-flex items-center gap-1 text-info" title={t('cvt.card_chip_title')}>
        <Repeat class="size-3" />{t('cvt.card_chip')}
      </span>
    {:else if compat.status === 'unsupported'}
      <span class="inline-flex items-center gap-1 text-warning">
        <Ban class="size-3" />{repo.not_runnable && ['audio_cpp_unsupported', 'vibevoice_layout', 'audio_cpp_incomplete_package', 'sdxl_refiner_folder_required'].includes(repo.not_runnable) ? t(notRunnableKey(repo.not_runnable)) : speechNeedsConversion(repo) ? t('hub.asr_needs_conversion') : compat.format === 'gguf' ? t('hub.compat_gguf_other_engine') : t('hub.compat_unsupported', { format: formatName(compat.format) })}
      </span>
    {:else if compat.status === 'unsupported_platform'}
      {@const msg = onnxUnsupportedText(compat)}
      <span class="inline-flex items-center gap-1 text-warning" title={t(msg.key, msg.params)}>
        <Ban class="size-3" />{t('ort.compat_unsupported')}
      </span>
    {:else if (compat.status === 'runnable' || compat.status === 'needs_runtime') && compat.runtime === 'crispasr'}
      <span class="inline-flex items-center gap-1 text-info">CrispASR</span>
    {:else if compat.status === 'runnable' && compat.runtime in MLX_CHIPS}
      {@const k = mlxChip(compat.runtime)}
      <span class="inline-flex items-center gap-1 text-success" title={t(k.title)}>
        <CircleCheck class="size-3" />{t(k.label)}
      </span>
    {:else if compat.status === 'needs_runtime' && compat.runtime in MLX_CHIPS}
      {@const k = mlxChip(compat.runtime)}
      <span class="inline-flex items-center gap-1 text-info" title={t(k.needsTitle)}>
        <Info class="size-3" />{t(k.needs)}
      </span>
    {:else if compat.status === 'runnable' && compat.runtime === 'python-hf'}
      {@const diffusers = repo.format === 'diffusers'}
      <span class="inline-flex items-center gap-1 text-success" title={t(diffusers ? 'hub.compat_pyhf_diffusers_title' : 'hub.compat_pyhf_title')}>
        <CircleCheck class="size-3" />{t(diffusers ? 'hub.compat_pyhf_diffusers' : 'hub.compat_pyhf')}
      </span>
    {:else if compat.status === 'needs_runtime' && compat.runtime === 'python-hf'}
      <span class="inline-flex items-center gap-1 text-info" title={t('hub.compat_needs_pyhf_title')}>
        <Info class="size-3" />{t('hub.compat_needs_pyhf')}
      </span>
    {:else if compat.status === 'needs_runtime' && compat.runtime === 'mistral.rs'}
      <span class="inline-flex items-center gap-1 text-info" title={t('hub.compat_needs_mistralrs_title')}>
        <Info class="size-3" />{t('hub.compat_needs_mistralrs')}
      </span>
    {:else if (compat.status === 'needs_runtime' || compat.status === 'runnable') && compat.runtime === 'audio.cpp'}
      <span class="inline-flex items-center gap-1 text-warning" title={t('audio_cpp.needs_engine')}><Info class="size-3" />{!app.settings?.audio_cpp_enabled ? t('audio_cpp.needs_engine') : engineInstall.installed('audio.cpp') ? t('process.experimental') : t('speak.engine_missing', { family: 'audio.cpp' })}</span>
    {:else if compat.status === 'needs_runtime'}
      {@const sd = compat.runtime === 'stable-diffusion.cpp'}
      <span class="inline-flex items-center gap-1 text-info" title={t(sd ? 'hub.compat_needs_sd_title' : 'hub.compat_needs_whisper_title')}>
        <Info class="size-3" />{t(sd ? 'hub.compat_needs_sd' : 'hub.compat_needs_whisper')}
      </span>
    {/if}
    {#if decision}
      <span class="inline-flex items-center gap-1 text-info" title={t('hub.badge_decision_title')}>
        <Info class="size-3" />{t('hub.badge_decision')}
      </span>
    {/if}
    {#if repo.pickle_weights}
      <span class="inline-flex items-center gap-1 text-muted-foreground" title={t('hub.pickle_weights_title')}>
        <Info class="size-3" />{t('hub.pickle_weights')}
      </span>
    {/if}
    {#if unsupported}
      <!-- the format reason above covers the task verdict -->
    {:else if taskNeeds && compat.status !== 'needs_runtime' && !((compat.status === 'runnable') && OWN_RUNTIMES.includes(compat.runtime))}
      <span class="inline-flex items-center gap-1 text-info" title={t('lib.runtime_needs_hint', { runtime: taskNeeds })}>
        <Info class="size-3" />{t('lib.runtime_needs', { runtime: taskNeeds })}
      </span>
    {:else if support === 'no' && task && !taskNeeds}
      <span class="inline-flex items-center gap-1 text-warning" title={t('hub.card_unsupported_title', { task: task.hf })}>
        <Ban class="size-3" />{t('hub.card_unsupported')}
      </span>
    {:else if support === 'partial' && task}
      <span class="text-muted-foreground" title={t('hub.card_partial_title', { task: task.hf })}>{t('hub.card_partial')}</span>
    {/if}
    {#if repo.adapter}
      <span class="inline-flex items-center gap-1 text-info" title={compat.status === 'unsupported' ? t('hub.badge_lora_diffusion_title') : repo.adapter_base ? t('hub.badge_lora_title', { base: repo.adapter_base }) : t('hub.badge_lora_title_nobase')}>
        <Info class="size-3" />{t('hub.badge_lora')}
      </span>
    {:else if repo.not_runnable && compat.status !== 'unsupported'}
      <span class="inline-flex items-center gap-1 text-warning" title={t(notRunnableKey(repo.not_runnable))}>
        <Ban class="size-3" />{t('hub.not_runnable')}
      </span>
    {/if}
    {#if inferred && task && uiLevel.advanced}
      <span class="text-muted-foreground" title={t(taskSourceKey(repo.inferred_source), { task: task.hf })}>{t('hub.card_task_inferred', { task: t(task.key) })}</span>
    {/if}
    {#if hasUpdate}
      <UpdateBadge />
    {/if}
    {#if repo.gated}
      <span
        class="inline-flex min-w-0 items-center gap-1 text-warning"
        title={license ? `${t('hfauth.gate_title')} · ${t('hfauth.gate_license', { license })}` : t('hfauth.gate_title')}
      >
        <span class="truncate">{t('hfauth.card_gated_badge')}{license ? ` · ${license}` : ''}</span>
      </span>
    {/if}
    {#if capLabels.length > 0}
      <span class="min-w-0 truncate text-muted-foreground" title={license ?? undefined}>{capLabels.join(' · ')}</span>
    {/if}
  </div>
  {#if quants.length > 0 && uiLevel.advanced}
    <div class="flex min-w-0 flex-wrap items-center gap-1 font-mono text-xs" aria-label={t('hub.quant_label')}>
      {#each quants.slice(0, 4) as q (q)}
        <span
          class={cn(
            'rounded border px-1.5 py-px',
            quantFilter === q ? 'border-foreground/40 bg-muted text-foreground' : 'border-border text-muted-foreground'
          )}>{q}</span
        >
      {/each}
      {#if quants.length > 4}
        <span class="text-muted-foreground">+{quants.length - 4}</span>
      {/if}
      {#if recommended}
        <span class="ml-auto inline-flex items-center gap-1 font-sans text-xs text-muted-foreground" title={t('hub.recommended_title')}>
          {t('hub.recommended_short', { quant: recommended })}
        </span>
      {/if}
    </div>
  {/if}
</button>
<span class="absolute right-1.5 top-1.5"><HelpPopover text={t('help.repo_select')} label={name} /></span>
</div>
