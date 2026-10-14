<script lang="ts">
  // CrispASR's helper models (language detection, punctuation): pinned, hash-checked files that
  // come through the download queue. The worker itself never downloads anything.
  import { onDestroy, onMount } from 'svelte';
  import { toast } from 'svelte-sonner';
  import { t } from '#lib/i18n.svelte.js';
  import { invoke } from '#lib/ipc.js';
  import { localizeError } from '#lib/network.js';
  import { formatBytes } from '#lib/format.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Download, Trash2 } from '@lucide/svelte';

  interface Helper {
    id: string;
    title: string;
    file: string;
    size_bytes: number;
    installed: boolean;
  }

  let helpers = $state<Helper[]>([]);
  let queued = $state<string[]>([]);
  let busy = $state(false);
  let timer: ReturnType<typeof setInterval> | undefined;

  async function refresh() {
    try {
      helpers = await invoke<Helper[]>('get_crispasr_helpers');
      queued = queued.filter((id) => !helpers.find((h) => h.id === id)?.installed);
    } catch {
      /* the list stays as it was */
    }
  }

  async function download(id: string) {
    busy = true;
    try {
      await invoke('download_crispasr_helper', { id });
      queued = [...queued, id];
      toast.success(t('crisp.helper_queued'));
    } catch (e) {
      toast.error(localizeError(e));
    } finally {
      busy = false;
    }
  }

  async function remove(id: string) {
    busy = true;
    try {
      await invoke('remove_crispasr_helper', { id });
      await refresh();
    } catch (e) {
      toast.error(localizeError(e));
    } finally {
      busy = false;
    }
  }

  const label = (id: string) => (id === 'lid' ? t('crisp.helper_lid') : t('crisp.helper_punc'));
  const note = (id: string) => (id === 'lid' ? t('crisp.helper_lid_note') : t('crisp.helper_punc_note'));

  onMount(() => {
    void refresh();
    timer = setInterval(() => queued.length && void refresh(), 3000);
  });
  onDestroy(() => timer && clearInterval(timer));
</script>

<div class="flex flex-col gap-1 py-3 pl-8 pr-4" data-testid="crispasr-helpers">
  <span class="text-sm font-medium">{t('crisp.helpers_title')}</span>
  <span class="text-xs text-muted-foreground">{t('crisp.helpers_desc')}</span>
  {#each helpers as h (h.id)}
    <div class="mt-1 flex min-w-0 items-center justify-between gap-3">
      <div class="flex min-w-0 flex-col gap-0.5">
        <span class="flex items-center gap-1.5 text-sm">
          {label(h.id)}
          <Badge variant={h.installed ? 'secondary' : 'outline'} class="text-xs">
            {h.installed ? t('crisp.helper_installed') : queued.includes(h.id) ? t('crisp.helper_downloading') : t('crisp.helper_missing')}
          </Badge>
        </span>
        <span class="text-xs text-muted-foreground">{note(h.id)} · {formatBytes(h.size_bytes, 1)}</span>
      </div>
      {#if h.installed}
        <Button size="sm" variant="outline" disabled={busy} onclick={() => remove(h.id)}><Trash2 />{t('crisp.helper_remove')}</Button>
      {:else}
        <Button size="sm" disabled={busy || queued.includes(h.id)} onclick={() => download(h.id)}><Download />{t('crisp.helper_download')}</Button>
      {/if}
    </div>
  {/each}
</div>
