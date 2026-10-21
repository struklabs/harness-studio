<script lang="ts">
  import { fixed } from '#lib/format.js';
  import { onDestroy, onMount } from 'svelte';
  import { isTauri } from '@tauri-apps/api/core';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { invoke } from '#lib/ipc.js';
  import { t } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import { errorText } from '#lib/errors.js';
  let { model }: { model: string } = $props();
  let documentData = $state('');
  let filename = $state('');
  let text = $state('');
  let question = $state('');
  let answer = $state('');
  let score = $state<number | null>(null);
  let error = $state('');
  let busy = $state(false);
  let picking = $state(false);
  let input: HTMLInputElement;
  let root: HTMLDivElement;
  let controller: AbortController | undefined;
  let selection = 0;
  onDestroy(() => { selection++; controller?.abort(); });
  onMount(() => {
    if (!isTauri()) return;
    const unlisten = getCurrentWebview().onDragDropEvent(async ({ payload }) => {
      if (payload.type !== 'drop' || !root?.offsetParent || busy || picking) return;
      const path = payload.paths[0];
      if (!path) return;
      const version = ++selection;
      picking = true; error = ''; text = ''; answer = ''; score = null; documentData = ''; filename = '';
      try {
        const data = await invoke<string>('read_ocr_file', { path });
        if (version === selection) { documentData = data; filename = path.split(/[/\\]/).pop() ?? path; }
      } catch (e) { if (version === selection) error = errorText(e); }
      finally { if (version === selection) picking = false; }
    });
    return () => { void unlisten.then(stop => stop()); };
  });
  async function pick(file?: File) {
    if (!file || busy) return;
    const version = ++selection;
    error = ''; text = ''; answer = ''; score = null; documentData = ''; filename = '';
    if (file.size > 20 * 1024 * 1024 || !['image/png', 'image/jpeg', 'image/webp', 'image/gif', 'application/pdf'].includes(file.type)) {
      error = t('ocr.bad_file'); return;
    }
    picking = true;
    try {
      const data = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(String(reader.result));
        reader.onerror = reject;
        reader.readAsDataURL(file);
      });
      if (version === selection) { documentData = data; filename = file.name; }
    } catch { if (version === selection) error = t('ocr.read_failed'); }
    finally { if (version === selection) picking = false; }
  }
  async function run(qa: boolean) {
    if (busy || !documentData || !model || !app.gatewayRunning) return;
    controller = new AbortController(); busy = true; error = '';
    if (qa) { answer = ''; score = null; } else text = '';
    try {
      const response = await fetch(`${app.inferenceUrl.replace(/\/$/, '')}/${qa ? 'document/qa' : 'ocr'}`, {
        method: 'POST', headers: { ...app.inferenceHeaders(), 'Content-Type': 'application/json' },
        body: JSON.stringify({ model, document: documentData, ...(qa ? { question } : {}) }), signal: controller.signal,
      });
      if (!response.ok) {
        const failure = await response.json().catch(() => null);
        throw new Error(errorText(failure?.error?.harness?.error_key ?? failure?.error?.message ?? `HTTP ${response.status}`));
      }
      const result = await response.json();
      if (qa) { answer = result.answer; score = typeof result.score === 'number' ? result.score : null; }
      else text = result.markdown;
    } catch (e) { if (!controller.signal.aborted) error = e instanceof Error ? e.message : t('ocr.read_failed'); }
    finally { busy = false; controller = undefined; }
  }
  async function copy() {
    try { await navigator.clipboard.writeText(text); } catch { error = t('ocr.copy_failed'); }
  }
</script>

<div bind:this={root} class="mx-auto flex w-full max-w-[860px] flex-col gap-4 px-4 py-6">
  <h2 class="text-lg font-semibold">{t('ocr.tab')}</h2>
  <p class="text-sm text-muted-foreground">{t('ocr.hint')}</p>
  {#if !app.gatewayRunning}<p role="status">{t('ocr.need_gateway')}</p>{/if}
  {#if !model}<p role="status">{t('ocr.need_model')}</p>{/if}
  <div role="region" aria-label={t('ocr.drop')} class="rounded-lg border border-dashed border-border p-6"
    ondragover={(e) => e.preventDefault()} ondrop={(e) => { e.preventDefault(); void pick(e.dataTransfer?.files[0]); }}>
    <p class="mb-3 text-sm">{filename || t('ocr.drop')}</p>
    <input bind:this={input} type="file" class="sr-only" accept="image/png,image/jpeg,image/webp,image/gif,application/pdf"
      onchange={(e) => { void pick(e.currentTarget.files?.[0]); e.currentTarget.value = ''; }} disabled={busy} aria-label={t('ocr.pick')} />
    <Button variant="outline" disabled={busy || picking} onclick={() => input.click()}>{t('ocr.pick')}</Button>
  </div>
  <Button disabled={busy || picking || !documentData || !model || !app.gatewayRunning} onclick={() => run(false)}>{t(busy ? 'ocr.busy' : 'ocr.read')}</Button>
  {#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
  {#if text}
    <label for="ocr-text" class="text-sm font-semibold">{t('ocr.text')}</label>
    <Textarea id="ocr-text" value={text} readonly rows={12} class="font-mono" />
    <Button variant="outline" onclick={copy}>{t('ocr.copy')}</Button>
  {/if}
  <label for="ocr-question" class="text-sm font-semibold">{t('ocr.question')}</label>
  <Textarea id="ocr-question" bind:value={question} disabled={busy} rows={2} />
  <Button disabled={busy || picking || !documentData || !model || !question.trim() || !app.gatewayRunning} onclick={() => run(true)}>{t('ocr.ask')}</Button>
  {#if answer}<div role="status"><h3 class="text-sm font-semibold">{t('ocr.answer')}</h3><p class="whitespace-pre-wrap">{answer}</p>{#if score !== null}<p>{t('pl.score')}: {fixed(score, 3)}</p>{/if}</div>{/if}
</div>
