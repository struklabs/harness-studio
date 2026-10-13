<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import { AudioLines, Square, X } from '@lucide/svelte';
  import { Button } from '$lib/components/ui/button';
  import { t } from '$lib/i18n.svelte';
  import { localizeError } from '$lib/network';
  import { RecordError } from './recorder';
  import { holdReleaseWhile } from '$lib/windowActivity';
  import { createRecordingSession, recordingClock, type RecordingState } from './recordingSession';
  let { onFile, contextKey, blocked = false }: { onFile: (file: File) => void; contextKey: string; blocked?: boolean } = $props();
  let recState = $state<RecordingState>('idle');
  let seconds = $state(0);
  let error = $state('');
  const mic = createRecordingSession({
    state: next => recState = next,
    seconds: next => { seconds = next; if (next >= 120) mic.finish(); },
    level: () => {},
    file: file => onFile(file),
    error: e => error = e instanceof RecordError && e.code !== 'failed' ? t(`asr.rec_${e.code}`) : t('asr.rec_failed', { error: localizeError(e) }),
  });
  $effect(() => { void contextKey; untrack(() => { mic.cancel(); error = ''; }); });
  $effect(() => { if (blocked) untrack(mic.cancel); });
  $effect(() => holdReleaseWhile('chat-audio', () => recState !== 'idle'));
  onDestroy(mic.cancel);
</script>
<Button type="button" variant="ghost" size="icon-xs" disabled={blocked || recState === 'starting'}
  aria-label={t(recState === 'recording' ? 'pg.audio_stop' : 'pg.audio_record')}
  title={t(recState === 'recording' ? 'pg.audio_stop' : 'pg.audio_record')}
  onclick={() => { error = ''; if (recState === 'recording') mic.finish(); else void mic.record(); }}>
  {#if recState === 'recording'}<Square />{:else}<AudioLines />{/if}
</Button>
{#if recState !== 'idle'}
  <span class="text-xs" role="status">{recordingClock(seconds)}</span>
  <Button type="button" variant="ghost" size="icon-xs" aria-label={t('pg.audio_cancel')} onclick={mic.cancel}><X /></Button>
{/if}
{#if error}<span class="text-xs text-destructive" role="alert">{error}</span>{/if}
