<script lang="ts">
  import GenerationField from './GenerationField.svelte';
  // "Clone a Voice": a short voice sample (uploaded or recorded), the permission box and an
  // optional transcript. Shown only for models that can clone; the sample is sent with the
  // request and not kept.
  import { t } from '#lib/i18n.svelte.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import AudioFilePicker from './AudioFilePicker.svelte';
  import VoiceConsentCheckbox from '../common/VoiceConsentCheckbox.svelte';
  import { MAX_SAMPLE_BYTES, MAX_SAMPLE_SECONDS, SAMPLE_RATE } from './voiceClone';
  import { formatBytes } from '#lib/format.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { security } from '#lib/stores/security.svelte.js';

  let { file = $bindable(null), consent = $bindable(false), text = $bindable(''), needsText = false, busy = false, engineOff = false }: {
    file?: File | null; consent?: boolean; text?: string; needsText?: boolean; busy?: boolean; engineOff?: boolean;
  } = $props();
  let recState = $state<'idle' | 'starting' | 'recording'>('idle');
  let tooBig = $derived(!!file && file.size > MAX_SAMPLE_BYTES);
</script>

<section class="flex flex-col gap-3 rounded-md border border-border px-3 py-3" aria-labelledby="tts-clone-title">
  <div class="flex flex-col gap-0.5">
    <h4 id="tts-clone-title" class="text-sm font-semibold text-foreground">{t('voice.title')}</h4>
    <p class="text-xs text-muted-foreground">{t('voice.desc', { seconds: MAX_SAMPLE_SECONDS, size: formatBytes(MAX_SAMPLE_BYTES) })}</p>
  </div>
  {#if engineOff}
    <div class="flex flex-wrap items-center gap-2 rounded-md border border-warning/40 px-3 py-2 text-xs text-warning" role="status">
      <span class="min-w-0 flex-1">{t('voice.engine_disabled')} {t('audio_cpp.needs_engine')}</span>
      <Button type="button" variant="outline" size="xs" onclick={() => security.openSettings('runtime-installed', t('audio_cpp.experimental'))}>{t('audio_cpp.enable')}</Button>
    </div>
  {/if}
  <GenerationField label={t('voice.sample_label')} id="tts-clone-file" help="gen.clone_sample">

    <AudioFilePicker
      id="tts-clone-file"
      label={t('voice.sample_label')}
      accept=".wav,.mp3,.flac,.ogg,audio/wav,audio/mpeg,audio/flac,audio/ogg"
      bind:file
      bind:recState
      {busy}
      sampleRate={SAMPLE_RATE}
    />
    {#if tooBig}<p class="text-xs text-destructive" role="alert">{t('voice.too_big', { size: formatBytes(MAX_SAMPLE_BYTES) })}</p>{/if}
  </GenerationField>
  <GenerationField label={needsText ? t('voice.text_label_required') : t('voice.text_label')} id="tts-clone-text" help="gen.clone_text">
    <Textarea id="tts-clone-text" bind:value={text} rows={2} maxlength={1000} placeholder={t('voice.text_placeholder')} disabled={busy} required={needsText} aria-required={needsText} />
  </GenerationField>
  <VoiceConsentCheckbox id="tts-clone-consent" bind:checked={consent} disabled={busy} />
</section>
