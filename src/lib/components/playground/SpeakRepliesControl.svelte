<script lang="ts">
  import { Volume2, VolumeOff } from '@lucide/svelte';
  import { Button } from '#lib/components/ui/button/index.js';
  import { t } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { readAloud, tts } from './tts.svelte';
  import { ttsRequirement } from './tts';
  import { speakBlocker } from './speakReplies';
  /** The selected model speaks natively: it answers aloud itself, no voice model is needed. */
  let { disabled = false, native = false }: { disabled?: boolean; native?: boolean } = $props();
  let missing = $derived(tts.overview ? ttsRequirement(tts.models, tts.model, tts.overview.runtime_installed) : null);
  let blocker = $derived(tts.overview ? speakBlocker(tts.models.length > 0, !missing, app.gatewayRunning) : null);
  $effect(() => { if (tts.speakReplies && !native && !tts.overview) void tts.load(); });
  function toggle() {
    const next = !tts.speakReplies;
    tts.setSpeakReplies(next);
    if (!next) readAloud.stop();
    else if (!native && !tts.overview) void tts.load();
  }
</script>
<Button type="button" variant={tts.speakReplies ? 'secondary' : 'ghost'} size="icon-xs" {disabled}
  aria-pressed={tts.speakReplies} aria-label={t('pg.speak_replies')} title={native ? t('pg.speak_replies_native') : t('pg.speak_replies')} onclick={toggle}>
  {#if tts.speakReplies}<Volume2 />{:else}<VolumeOff />{/if}
</Button>
{#if tts.speakReplies && native}
  <span class="text-xs text-muted-foreground">{t('pg.speak_replies_native')}</span>
{:else if tts.speakReplies && tts.models.length > 0}
  <select class="h-7 max-w-36 rounded-md border border-input bg-background px-1.5 text-xs text-foreground"
    aria-label={t('pg.speak_replies_voice')} title={t('pg.speak_replies_voice')} {disabled}
    bind:value={tts.model} onchange={() => { tts.voice = ''; }}>
    {#each tts.models as m (m.id)}<option value={m.name}>{m.name}</option>{/each}
  </select>
{:else if tts.speakReplies && tts.overview}
  <span class="text-xs text-muted-foreground">{t('speak.no_model_chat')}</span>
{/if}
{#if tts.speakReplies && !native && blocker && blocker !== 'no_model'}
  <span class="text-xs text-muted-foreground">{blocker === 'engine' ? t('speak.engine_missing', { family: missing ?? '' }) : t('speak.need_gateway')}</span>
{/if}
