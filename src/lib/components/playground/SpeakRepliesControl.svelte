<script lang="ts">
  import { Volume2, VolumeOff } from '@lucide/svelte';
  import { Button } from '#lib/components/ui/button/index.js';
  import { t } from '#lib/i18n.svelte.js';
  import { readAloud, tts } from './tts.svelte';
  let { disabled = false }: { disabled?: boolean } = $props();
  $effect(() => { if (tts.speakReplies && !tts.overview) void tts.load(); });
  function toggle() {
    const next = !tts.speakReplies;
    tts.setSpeakReplies(next);
    if (!next) readAloud.stop();
    else if (!tts.overview) void tts.load();
  }
</script>
<Button type="button" variant={tts.speakReplies ? 'secondary' : 'ghost'} size="icon-xs" {disabled}
  aria-pressed={tts.speakReplies} aria-label={t('pg.speak_replies')} title={t('pg.speak_replies')} onclick={toggle}>
  {#if tts.speakReplies}<Volume2 />{:else}<VolumeOff />{/if}
</Button>
{#if tts.speakReplies && tts.models.length > 0}
  <select class="h-7 max-w-36 rounded-md border border-input bg-background px-1.5 text-xs text-foreground"
    aria-label={t('pg.speak_replies_voice')} title={t('pg.speak_replies_voice')} {disabled}
    bind:value={tts.model} onchange={() => { tts.voice = ''; }}>
    {#each tts.models as m (m.id)}<option value={m.name}>{m.name}</option>{/each}
  </select>
{:else if tts.speakReplies && tts.overview}
  <span class="text-xs text-muted-foreground">{t('speak.no_model_chat')}</span>
{/if}
