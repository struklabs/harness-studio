<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { errorText, rawError } from '#lib/errors.js';
  import { locale, t } from '#lib/i18n.svelte.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import SettingGroup from './SettingGroup.svelte';

  interface Checkpoint {
    id: string; group: string; tool: string; path: string; created: boolean;
    bytes_before: number; restorable: boolean; at_ms: number; undone: boolean;
  }

  let items = $state<Checkpoint[]>([]);
  let error = $state('');
  let pending = $state(false);
  /** Checkpoint ids whose undo was refused because the file changed since (offer the forced undo). */
  let changed = $state<Record<string, boolean>>({});

  async function load() {
    try { items = await invoke<Checkpoint[]>('checkpoints_list'); }
    catch (e) { error = errorText(e); }
  }
  onMount(() => { void load(); });

  async function undo(c: Checkpoint, force: boolean) {
    pending = true; error = '';
    try { await invoke('checkpoints_undo', { id: c.id, force }); delete changed[c.id]; }
    catch (e) {
      if (rawError(e).startsWith('checkpoint.changed_since')) changed[c.id] = true;
      error = errorText(e);
    } finally { pending = false; await load(); }
  }
  async function undoRun(group: string) {
    pending = true; error = '';
    try { await invoke('checkpoints_undo_run', { group, force: false }); }
    catch (e) { error = errorText(e); }
    finally { pending = false; await load(); }
  }
  async function clear() {
    pending = true; error = '';
    try { await invoke('checkpoints_clear'); }
    catch (e) { error = errorText(e); }
    finally { pending = false; await load(); }
  }
  const groups = $derived([...new Set(items.filter((c) => !c.undone && c.restorable).map((c) => c.group))]);
  const when = (c: Checkpoint) => new Date(c.at_ms).toLocaleString(locale());
</script>

<div class="flex flex-col gap-3" data-testid="agent-checkpoints">
  <div class="flex flex-col gap-1">
    <h3 class="text-sm font-semibold">{t('ck.title')}</h3>
    <p class="text-xs text-muted-foreground">{t('ck.intro')}</p>
  </div>
  {#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
  <SettingGroup title={t('ck.title')}>
    <div class="flex flex-col gap-2 px-4 py-3">
      {#if groups.length}
        <div class="flex flex-wrap gap-2">
          {#each groups as g (g)}
            <Button variant="outline" size="sm" disabled={pending} onclick={() => undoRun(g)} title={t('ck.run', { group: g })}>
              {t('ck.undo_run')} · <span class="max-w-40 truncate">{g}</span>
            </Button>
          {/each}
        </div>
      {/if}
      {#each items as c (c.id)}
        <div class="flex flex-wrap items-center gap-2 rounded border px-2 py-1 text-xs" data-checkpoint={c.id}>
          <Badge variant="secondary">{c.created ? t('ck.created') : t('ck.changed')}</Badge>
          <span class="break-all font-mono">{c.path}</span>
          <span class="text-muted-foreground">{c.tool} · {when(c)}</span>
          {#if c.undone}<Badge variant="outline">{t('ck.undone')}</Badge>{/if}
          {#if !c.restorable && !c.undone}<Badge variant="outline">{t('ck.too_large')}</Badge>{/if}
          <span class="flex-1"></span>
          {#if !c.undone && c.restorable}
            {#if changed[c.id]}
              <span class="text-muted-foreground">{t('ck.changed_since_hint')}</span>
              <Button variant="destructive" size="sm" disabled={pending} onclick={() => undo(c, true)}>{t('ck.undo_anyway')}</Button>
            {:else}
              <Button variant="outline" size="sm" disabled={pending} onclick={() => undo(c, false)}>{t('ck.undo')}</Button>
            {/if}
          {/if}
        </div>
      {:else}
        <p class="text-xs text-muted-foreground">{t('ck.none')}</p>
      {/each}
      {#if items.length}
        <div><Button variant="outline" size="sm" disabled={pending} onclick={clear}>{t('ck.clear')}</Button></div>
      {/if}
    </div>
  </SettingGroup>
</div>
