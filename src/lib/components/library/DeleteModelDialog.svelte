<script lang="ts">
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  // Deleting a model file: lists every part with its size, the definitions and profiles that
  // use it, and refuses while a server uses it. Linked (Ollama / LM Studio) models only lose
  // their link; the file in the other app is never touched.
  import { invoke } from '#lib/ipc.js';
  import { onMount } from 'svelte';
  import type { DeletePreview, DeleteReport, UseRole } from '#lib/types.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { formatBytes } from '#lib/format.js';
  import { app } from '#lib/stores/app.svelte.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Checkbox } from '#lib/components/ui/checkbox/index.js';

  interface Props {
    modelId: string;
    onDone: (message: string) => void;
    onClose: () => void;
  }

  let { modelId, onDone, onClose }: Props = $props();

  const uid = $props.id();
  const ROLE: Record<UseRole, TKey> = {
    base: 'lib.role_base',
    draft: 'lib.role_draft',
    mmproj: 'lib.role_mmproj',
    adapter: 'lib.role_adapter',
  };

  let preview = $state<DeletePreview | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let withDefinitions = $state(false);
  let withCompanions = $state(true);

  onMount(async () => {
    try {
      preview = await invoke<DeletePreview>('get_model_delete_preview', { modelId });
    } catch (e) {
      error = String(e);
    }
  });

  let total = $derived(preview?.files.reduce((n, f) => n + f.size_bytes, 0) ?? 0);
  let blocked = $derived(!!preview && preview.running_on.length > 0);
  let companions = $derived(preview?.companions ?? []);
  let companionBytes = $derived(companions.reduce((n, f) => n + f.size_bytes, 0));
  /** The companions go too while their box is ticked: the count and size say so. */
  let takeCompanions = $derived(withCompanions && companions.length > 0);
  let baseDefs = $derived(preview?.definitions.filter((d) => d.role === 'base') ?? []);

  async function stopServers() {
    if (!preview || busy) return;
    busy = true;
    error = null;
    try {
      const results = await Promise.allSettled(
        preview.running_on.map((serverId) => invoke('stop_server', { serverId })),
      );
      preview = await invoke<DeletePreview>('get_model_delete_preview', { modelId });
      await app.refreshServer();
      const failure = results.find((result) => result.status === 'rejected');
      if (failure?.status === 'rejected') error = localizeError(failure.reason);
    } catch (e) {
      error = localizeError(e);
    } finally {
      busy = false;
    }
  }

  async function confirm() {
    if (!preview) return;
    busy = true;
    error = null;
    try {
      if (preview.linked) {
        await invoke('unlink_external_model', { modelId });
        onDone(t('lib.unlinked', { name: preview.name }));
      } else {
        const report = await invoke<DeleteReport>('delete_library_model', { modelId, deleteDefinitions: withDefinitions, deleteCompanions: takeCompanions });
        let msg = t('lib.deleted', { name: preview.name, count: report.files.length });
        if (report.definitions_deleted.length) msg += ' ' + t('lib.deleted_defs', { aliases: report.definitions_deleted.join(', ') });
        if (report.definition_errors.length) msg += ' ' + t('lib.deleted_defs_failed', { errors: report.definition_errors.map(localizeError).join('; ') });
        onDone(msg);
      }
    } catch (e) {
      error = localizeError(e);
    } finally {
      busy = false;
    }
  }
</script>

<Dialog.Root open={true} onOpenChange={(open) => !open && !busy && onClose()}>
  <Dialog.Content class="flex max-h-[85vh] flex-col sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title>
        {#if preview?.linked}{t('lib.unlink_title', { name: preview.name })}{:else}{t('lib.delete_title', { name: preview?.name ?? '' })}{/if}
      </Dialog.Title>
      <Dialog.Description>
        {#if !preview}{t('lib.delete_loading')}
        {:else if blocked}{t('lib.delete_blocked')}
        {:else if preview.linked}{t('lib.unlink_desc')}
        {:else}{t('lib.delete_desc', { size: formatBytes(total + (takeCompanions ? companionBytes : 0)) })}{/if}
      </Dialog.Description>
    </Dialog.Header>

    {#if preview}
      <div class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto text-xs">
        {#if blocked}
          <ul class="flex flex-col gap-1 rounded-md border border-destructive/30 bg-destructive/10 p-2 text-destructive">
            {#each preview.running_on as id (id)}<li>{t('lib.running_server', { server: app.serverLabel(id) })}</li>{/each}
          </ul>
        {/if}
        <section class="flex flex-col gap-1">
          <span class="font-medium text-muted-foreground">{t('lib.delete_files', { count: preview.files.length })}</span>
          <ul class="flex flex-col gap-0.5 rounded-md border border-border p-2">
            {#each preview.files as f (f.path)}
              <li class="flex min-w-0 items-center gap-2">
                <span class="min-w-0 flex-1 truncate font-mono text-xs" title={f.path}>{f.path}</span>
                <span class="shrink-0 font-num text-muted-foreground">{formatBytes(f.size_bytes)}</span>
              </li>
            {/each}
          </ul>
        </section>
        {#if companions.length && !preview.linked}
          <section class="flex flex-col gap-1">
            <span class="font-medium text-muted-foreground">{t('lib.delete_companions', { count: companions.length })}</span>
            <ul class="flex flex-col gap-0.5 rounded-md border border-border p-2">
              {#each companions as f (f.path)}
                <li class="flex min-w-0 items-center gap-2">
                  <span class="min-w-0 flex-1 truncate font-mono text-xs" title={f.path}>{f.path}</span>
                  <span class="shrink-0 font-num text-muted-foreground">{formatBytes(f.size_bytes)}</span>
                </li>
              {/each}
            </ul>
            {#if !blocked}
              <label class="flex items-center gap-2" for="{uid}-companions">
                <FieldHelp text={t('help.delete_companions')}><Checkbox id="{uid}-companions" bind:checked={withCompanions} /></FieldHelp>
                <span>{t('lib.delete_with_companions', { size: formatBytes(companionBytes) })}</span>
              </label>
            {/if}
          </section>
        {/if}
        {#if preview.definitions.length}
          <section class="flex flex-col gap-1">
            <span class="font-medium text-muted-foreground">{t('lib.delete_used_by')}</span>
            <ul class="flex flex-col gap-0.5 rounded-md border border-border p-2">
              {#each preview.definitions as d (d.id)}
                <li class="flex items-center gap-2"><span class="truncate font-mono">{d.alias}</span><span class="text-muted-foreground">{t(ROLE[d.role])}</span></li>
              {/each}
            </ul>
            <p class="text-muted-foreground">{t('lib.delete_defs_note')}</p>
          </section>
        {/if}
        {#if preview.profiles.length}
          <section class="flex flex-col gap-1">
            <span class="font-medium text-muted-foreground">{t('lib.delete_profiles')}</span>
            <ul class="flex flex-col gap-0.5 rounded-md border border-border p-2">
              {#each preview.profiles as p (p.id)}<li class="truncate">{p.name} <span class="font-mono text-muted-foreground">{p.id}</span></li>{/each}
            </ul>
          </section>
        {/if}
        {#if !preview.linked && baseDefs.length > 0 && !blocked}
          <label class="flex items-center gap-2" for="{uid}-defs">
            <FieldHelp text={t('help.delete_defs')}><Checkbox id="{uid}-defs" bind:checked={withDefinitions} /></FieldHelp>
            <span>{t('lib.delete_with_defs', { count: baseDefs.length })}</span>
          </label>
        {/if}
      </div>
    {/if}

    {#if error}
      <p class="rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-2 text-xs text-destructive" role="alert">{error}</p>
    {/if}
    <Dialog.Footer>
      <Button variant="outline" onclick={onClose} disabled={busy}>{blocked ? t('def.close') : t('ip.cancel')}</Button>
      {#if blocked}
        <Button onclick={stopServers} disabled={busy}>{t('lib.stop_server')}</Button>
      {/if}
      {#if preview && !blocked}
        <Button variant="destructive" onclick={confirm} disabled={busy}>
          {preview.linked ? t('lib.unlink') : t('lib.delete_confirm', { count: preview.files.length + (takeCompanions ? companions.length : 0) })}
        </Button>
      {/if}
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
