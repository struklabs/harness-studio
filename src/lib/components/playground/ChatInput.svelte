<script lang="ts">
  import { thinkingDefaultBudget, thinkingOptions, type ThinkingLevel, type ReasoningControl, NO_REASONING } from './thinking';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { t } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Kbd } from '#lib/components/ui/kbd/index.js';
  import * as InputGroup from '#lib/components/ui/input-group/index.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import { ArrowUp, Paperclip, Square, Globe, ImagePlus, FileMusic, X } from '@lucide/svelte';
  import type { PgImage } from './images';
  import { audioUrl, type PgAudio } from './audio';
  import AudioRecordButton from './AudioRecordButton.svelte';
  import DictationButton from './DictationButton.svelte';
  import { appendTranscript } from './recordingSession';
  import { toastClearance } from '#lib/toastClearance.js';
  import type { Snippet } from 'svelte';

  interface Props {
    value: string;
    /** A reply is being generated: the send button turns into Stop. */
    streaming: boolean;
    /** No running server: the composer is read-only. */
    disabled: boolean;
    /** Wait for saved preferences and the offered tool registry before sending. */
    sendBlocked?: boolean;
    /** A tool call is waiting for the user's decision. */
    awaitingApproval?: boolean;
    onSend: () => void;
    onStop: () => void;
    /** Attach documents to the conversation (hidden when not given). */
    onAttach?: () => void;
    /** Pictures waiting to be sent with the message. */
    images?: PgImage[];
    audio?: PgAudio[];
    onAudioFile?: (file: File) => void;
    onPickAudio?: () => void;
    onRemoveAudio?: (index: number) => void;
    audioContextKey?: string;
    /** Picture files chosen with the button or pasted (the button is hidden when not given). */
    onImageFiles?: (files: File[]) => void;
    /** The loaded model cannot read pictures: the button is off and `imagesBlockedHint` says why. */
    imagesBlocked?: boolean;
    imagesBlockedHint?: string;
    /** Opens the native picture chooser (the webview's file input does not open one in the app). */
    onPickImages?: () => void;
    onRemoveImage?: (index: number) => void;
    /** Shown instead of "Generating…" (e.g. while documents are searched). */
    status?: string;
    /** More composer actions, after the attach button (e.g. compaction). */
    actions?: Snippet;
    reasoningControl?: ReasoningControl;
    thinking?: ThinkingLevel;
    thinkingBudget?: number | null;
    presetThinking?: boolean | null;
    toolsEnabled: boolean;
    toolsBusy?: boolean;
    toolsHint: string;
    onToggleTools: () => void;
    contextKey: number;
  }

  let { value = $bindable(), streaming, disabled, sendBlocked = false, awaitingApproval = false, onSend, onStop, onAttach, images = [], audio = [], onAudioFile, onPickAudio, onRemoveAudio, audioContextKey = '', onImageFiles, imagesBlocked = false, imagesBlockedHint = '', onPickImages, onRemoveImage, status, actions, reasoningControl = NO_REASONING, thinking = $bindable('auto'), thinkingBudget = $bindable(null), presetThinking, toolsEnabled, toolsBusy = false, toolsHint, onToggleTools, contextKey }: Props = $props();

  let shownThinking = $derived(reasoningControl.effort && thinking === 'off' ? 'light' : thinking);
  let thinkingOn = $derived(reasoningControl.effort ? shownThinking !== 'light' : thinking === 'auto' ? presetThinking !== false : thinking !== 'off');
  let levelOptions = $derived(thinkingOptions(reasoningControl));
  function levelLabel(level: ThinkingLevel) {
    if (level === 'auto') return t('pg.thinking_auto');
    if (level === 'normal' && !reasoningControl.effort && !reasoningControl.budget) return t('pg.thinking_on');
    return t(`pg.thinking_${level}`);
  }

  let el = $state<HTMLTextAreaElement | null>(null);
  const uid = $props.id();
  const inputId = `pg-input-${uid}`;
  const hintId = `pg-hint-${uid}`;

  function autosize() {
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 208)}px`;
  }

  // Also re-fit when the value is replaced from outside (new chat, edit last message).
  $effect(() => {
    void value;
    autosize();
  });

  /** Focuses the textarea; `end` puts the caret after the text. */
  export function focus(end = false) {
    el?.focus();
    if (end && el) el.setSelectionRange(el.value.length, el.value.length);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      if (canSend) onSend();
    }
  }

  let canSend = $derived(!disabled && !sendBlocked && !streaming && (value.trim().length > 0 || images.length > 0 || audio.length > 0));

  let imageInput = $state<HTMLInputElement | null>(null);

  function onPaste(e: ClipboardEvent) {
    if (!onImageFiles) return;
    const files = [...(e.clipboardData?.files ?? [])].filter((f) => f.type.startsWith('image/'));
    if (!files.length) return;
    e.preventDefault(); // A pasted picture is not text.
    onImageFiles(files);
  }

  function onPicked(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const files = [...(input.files ?? [])];
    input.value = ''; // The same file can be chosen again.
    if (files.length) onImageFiles?.(files);
  }
</script>

<form
  class="flex flex-col gap-1.5"
  {@attach toastClearance}
  onsubmit={(e) => {
    e.preventDefault();
    if (canSend) onSend();
  }}
>
  <label for={inputId} class="sr-only">{t('pg.input_label')}</label>
  <InputGroup.Root class="bg-card">
    {#if images.length}
      <InputGroup.Addon align="block-start" class="flex-wrap gap-2" data-image-strip>
        {#each images as img, i (i)}
          <span class="group/thumb relative size-14 shrink-0 overflow-hidden rounded-md border border-border bg-muted" data-image-thumb>
            <img src={img.data} alt={img.name} title={img.name} class="size-full object-cover" />
            <button
              type="button"
              class="absolute top-0.5 right-0.5 flex size-4 cursor-pointer items-center justify-center rounded-full bg-background/80 text-foreground opacity-0 outline-none transition-opacity group-hover/thumb:opacity-100 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring"
              aria-label={t('pg.image_remove', { name: img.name })}
              onclick={() => onRemoveImage?.(i)}
            >
              <X class="size-3" />
            </button>
          </span>
        {/each}
      </InputGroup.Addon>
    {/if}
    {#if audio.length}
      <InputGroup.Addon align="block-start" class="flex-wrap gap-2">
        {#each audio as clip, i (i)}
          <span class="flex flex-wrap items-center gap-2 rounded-md border p-2">
            <span class="text-xs">{clip.name}</span>
            <audio controls preload="none" src={audioUrl(clip)} aria-label={clip.name}></audio>
            <Button type="button" variant="ghost" size="icon-xs" aria-label={t('pg.audio_remove', { name: clip.name })} onclick={() => onRemoveAudio?.(i)}><X /></Button>
          </span>
        {/each}
      </InputGroup.Addon>
    {/if}
    <InputGroup.Textarea
      id={inputId}
      bind:ref={el}
      bind:value
      rows={1}
      placeholder={disabled ? t('pg.input_placeholder_offline') : t('pg.input_placeholder')}
      {disabled}
      onkeydown={onKeydown}
      onpaste={onPaste}
      oninput={autosize}
      aria-describedby={hintId}
      class="max-h-52 min-h-10 text-sm"
    />
    <InputGroup.Addon align="block-end" class="justify-between flex-wrap gap-1">
      <span class="flex min-w-0 flex-1 flex-wrap items-center gap-y-1">
        {#if onAttach}
          <Tooltip.Root>
            <Tooltip.Trigger>
              {#snippet child({ props })}
                <Button {...props} type="button" variant="ghost" size="icon-xs" class="mr-1" aria-label={t('doc.attach')} onclick={(e) => {
                  // Focus returning from the file dialog would reopen the tooltip over the new chips.
                  e.currentTarget.blur();
                  onAttach?.();
                }}>
                  <Paperclip />
                </Button>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content>{t('doc.attach_tooltip')}</Tooltip.Content>
          </Tooltip.Root>
        {/if}
        {#if onImageFiles}
          <input bind:this={imageInput} type="file" accept="image/*" multiple class="hidden" tabindex="-1" aria-hidden="true" onchange={onPicked} />
          <Tooltip.Root>
            <Tooltip.Trigger>
              {#snippet child({ props })}
                <!-- aria-disabled, not disabled: a disabled button shows no tooltip. -->
                <Button {...props} type="button" variant="ghost" size="icon-xs" class={imagesBlocked ? 'mr-1 opacity-50' : 'mr-1'}
                  aria-label={t('pg.image_attach')} aria-disabled={imagesBlocked || undefined} data-image-attach
                  onclick={(e) => {
                    e.currentTarget.blur();
                    if (imagesBlocked) return;
                    if (onPickImages) onPickImages();
                    else imageInput?.click();
                  }}>
                  <ImagePlus />
                </Button>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content class="max-w-xs">{imagesBlocked ? imagesBlockedHint : t('pg.image_attach_tooltip')}</Tooltip.Content>
          </Tooltip.Root>
        {/if}
        <Tooltip.Root>
          <Tooltip.Trigger>
            {#snippet child({ props })}
              <Button {...props} type="button" size="sm" variant={toolsEnabled ? 'secondary' : 'ghost'}
                aria-pressed={toolsEnabled} disabled={streaming || toolsBusy} onclick={onToggleTools}>
                <Globe />{t('pg.web_tools')} · {t(toolsEnabled ? 'pg.web_tools_on' : 'pg.web_tools_off')}
              </Button>
            {/snippet}
          </Tooltip.Trigger>
          <Tooltip.Content class="max-w-sm">{toolsHint}</Tooltip.Content>
        </Tooltip.Root>
        {#if reasoningControl.effort || reasoningControl.toggle}
          {#if uiLevel.advanced}
            <label class="flex items-center gap-1 text-xs">
              <span>{t('pg.thinking_control')}</span>
              <select class="h-8 rounded-md border border-input bg-background px-2 text-xs" aria-label={t('pg.thinking_control')}
                disabled={streaming} value={levelOptions.includes(shownThinking) ? shownThinking : 'normal'}
                onchange={(event) => { thinking = event.currentTarget.value as ThinkingLevel; thinkingBudget = null; }}>
                {#each levelOptions as level}<option value={level}>{levelLabel(level)}</option>{/each}
              </select>
            </label>
            {#if reasoningControl.budget && thinking !== 'off'}
              <input type="number" min="-1" step="1" class="h-8 w-20 rounded-md border border-input bg-background px-2 text-xs"
                aria-label={t('pg.thinking_budget')} title={t('pg.thinking_budget')} placeholder={thinkingDefaultBudget(thinking) === -1 ? t('pg.thinking_unlimited') : String(thinkingDefaultBudget(thinking) ?? t('pg.thinking_auto'))}
                disabled={streaming} value={thinkingBudget ?? ''}
                oninput={(event) => { const value = event.currentTarget.value; thinkingBudget = value === '' ? null : Math.max(-1, Math.trunc(Number(value))); }} />
            {/if}
          {:else}
            <Button type="button" size="sm" variant={thinkingOn ? 'secondary' : 'ghost'} aria-label={t('pg.think')}
              aria-pressed={thinkingOn} disabled={streaming}
              onclick={() => { thinking = reasoningControl.effort ? (shownThinking === 'light' ? 'normal' : 'light') : (thinkingOn ? 'off' : 'normal'); thinkingBudget = null; }}>
              {t('pg.think')}{#if reasoningControl.effort} · {levelLabel(shownThinking === 'auto' ? 'normal' : shownThinking)}{/if}
            </Button>
          {/if}
        {/if}
        {#if onAudioFile}
          <Button type="button" variant="ghost" size="icon-xs" disabled={disabled || streaming} aria-label={t('pg.audio_attach')} title={t('pg.audio_attach')} onclick={onPickAudio}><FileMusic /></Button>
          <AudioRecordButton contextKey={audioContextKey} blocked={disabled || streaming} onFile={onAudioFile} />
        {/if}
        <DictationButton {contextKey} blocked={streaming} onText={(text) => {
          value = appendTranscript(value, text);
          focus(true);
        }} />
        {@render actions?.()}
        <span id={hintId} class={uiLevel.advanced ? "flex min-w-0 flex-wrap items-center gap-1 text-xs font-normal" : "sr-only"}>
          <Kbd>Enter</Kbd>
          {t('pg.hint_send')}
          <span aria-hidden="true">·</span>
          <Kbd>Shift</Kbd>+<Kbd>Enter</Kbd>
          {t('pg.hint_newline')}
        </span>
      </span>
      <span class="flex items-center gap-2">
        {#if awaitingApproval}
          <span class="text-xs text-warning" role="status">{t('pg.awaiting_approval')}</span>
        {:else if streaming}
          <span class="text-xs text-muted-foreground" role="status">{status || t('pg.generating')}</span>
        {/if}
        {#if streaming}
          <Button type="button" variant="outline" size="sm" onclick={onStop} aria-label={t('pg.stop')}>
            <Square class="fill-current" />
            {t('pg.stop')}
          </Button>
        {:else}
          <Tooltip.Root>
            <Tooltip.Trigger>
              {#snippet child({ props })}
                <Button {...props} type="submit" size="icon-sm" disabled={!canSend} aria-label={t('pg.send')}>
                  <ArrowUp />
                </Button>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content>{t('pg.send')}</Tooltip.Content>
          </Tooltip.Root>
        {/if}
      </span>
    </InputGroup.Addon>
  </InputGroup.Root>
</form>
