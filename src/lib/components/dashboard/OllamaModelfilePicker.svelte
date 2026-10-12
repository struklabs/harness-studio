<script lang="ts">
  import FieldHelp from '$lib/components/common/FieldHelp.svelte';
  // "Import from an installed Ollama model": lists the GGUF models in the Ollama models folder
  // (read-only, as `harness import modelfile --ollama <name>`) and previews `FROM <name>` with
  // the same dialog as a Modelfile import.
  import { invoke } from '$lib/ipc';
  import { onMount } from 'svelte';
  import type { ModelfileImport } from '$lib/types';
  import { localizeError } from '$lib/network';
  import { formatBytes } from '$lib/format';
  import { t } from '$lib/i18n.svelte';
  import * as Dialog from '$lib/components/ui/dialog';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Skeleton } from '$lib/components/ui/skeleton';
  import { CircleAlert, Eye, LoaderCircle } from '@lucide/svelte';

  interface OllamaInstalledModel {
    name: string;
    size_bytes: number;
    has_projector: boolean;
    quantization: string | null;
  }
  interface OllamaInstalledModels {
    dir: string | null;
    models: OllamaInstalledModel[];
  }

  interface Props {
    onPreview: (preview: ModelfileImport) => void;
    onClose: () => void;
  }

  let { onPreview, onClose }: Props = $props();

  const uid = $props.id();
  let list = $state<OllamaInstalledModels | null>(null);
  let loadError = $state<string | null>(null);
  let filter = $state('');
  let busyName = $state<string | null>(null);
  let error = $state<string | null>(null);

  let shown = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    const models = list?.models ?? [];
    return q ? models.filter((m) => m.name.toLowerCase().includes(q)) : models;
  });

  onMount(async () => {
    try {
      list = await invoke<OllamaInstalledModels>('list_ollama_models');
    } catch (e) {
      loadError = localizeError(e);
    }
  });

  async function pick(name: string) {
    busyName = name;
    error = null;
    try {
      onPreview(await invoke<ModelfileImport>('preview_ollama_import', { name }));
    } catch (e) {
      error = localizeError(e);
    } finally {
      busyName = null;
    }
  }
</script>

<Dialog.Root open={true} onOpenChange={(open) => !open && !busyName && onClose()}>
  <Dialog.Content class="flex max-h-[85vh] flex-col gap-0 p-0 sm:max-w-xl" aria-busy={!list || busyName != null}>
    <Dialog.Header class="border-b border-border p-4 pr-12">
      <Dialog.Title>{t('mf.ollama_title')}</Dialog.Title>
      <Dialog.Description>{t('mf.ollama_desc')}</Dialog.Description>
    </Dialog.Header>
    <div class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4 text-xs">
      {#if loadError}
        <div class="flex gap-2 rounded-lg border border-destructive/30 bg-destructive/10 p-2.5 text-destructive" role="alert">
          <CircleAlert class="mt-px size-3.5 shrink-0" /><span class="min-w-0 break-words">{loadError}</span>
        </div>
      {:else if !list}
        <div class="flex flex-col gap-2" role="status" aria-label={t('mf.ollama_loading')}>
          <Skeleton class="h-9 w-full" />
          <Skeleton class="h-9 w-full" />
        </div>
      {:else if !list.dir}
        <p class="py-4 text-center text-muted-foreground">{t('mf.ollama_no_dir')}</p>
      {:else}
        <p class="text-muted-foreground">
          {t('mf.ollama_folder')} <code class="font-mono break-all text-foreground">{list.dir}</code>
        </p>
        {#if list.models.length === 0}
          <p class="py-4 text-center text-muted-foreground">{t('mf.ollama_empty')}</p>
        {:else}
          {#if list.models.length > 6}
            <label class="sr-only" for="{uid}-filter">{t('mf.ollama_filter')}</label>
            <FieldHelp text={t('help.ollama_filter')}><Input id="{uid}-filter" bind:value={filter} placeholder={t('mf.ollama_filter')} class="h-8 text-xs" /></FieldHelp>
          {/if}
          <ul class="divide-y divide-border overflow-hidden rounded-lg border border-border">
            {#each shown as m (m.name)}
              <li class="flex items-center gap-3 px-3 py-2">
                <span class="min-w-0 flex-1 truncate font-mono" title={m.name}>{m.name}</span>
                <span class="flex shrink-0 items-center gap-2 text-muted-foreground">
                  {#if m.has_projector}<Eye class="size-3.5" aria-label={t('mf.ollama_vision')} />{/if}
                  {#if m.quantization}<span class="font-mono">{m.quantization}</span>{/if}
                  <span class="w-16 text-right font-num">{formatBytes(m.size_bytes)}</span>
                </span>
                <Button size="sm" variant="outline" disabled={busyName != null} onclick={() => pick(m.name)}>
                  {#if busyName === m.name}<LoaderCircle class="animate-spin" />{/if}{t('mf.ollama_pick')}
                </Button>
              </li>
            {:else}
              <li class="px-3 py-3 text-center text-muted-foreground">{t('mf.ollama_no_match')}</li>
            {/each}
          </ul>
          <p class="text-muted-foreground">{t('mf.ollama_gguf_only')}</p>
        {/if}
      {/if}
      {#if error}
        <div class="flex gap-2 rounded-lg border border-destructive/30 bg-destructive/10 p-2.5 text-destructive" role="alert">
          <CircleAlert class="mt-px size-3.5 shrink-0" /><span class="min-w-0 break-words">{error}</span>
        </div>
      {/if}
    </div>
    <Dialog.Footer class="border-t border-border p-4">
      <Button variant="outline" disabled={busyName != null} onclick={onClose}>{t('ip.cancel')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
