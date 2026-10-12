<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '$lib/ipc';
  import { listen } from '@tauri-apps/api/event';
  import { toast } from 'svelte-sonner';
  import { ShieldAlert, FileWarning } from '@lucide/svelte';
  import type { AppSettings } from '$lib/types';
  import { t } from '$lib/i18n.svelte';
  import { formatBytes } from '$lib/format';
  import { localizeError } from '$lib/network';
  import { app } from '$lib/stores/app.svelte';
  import { security, type ExposureBlocked } from '$lib/stores/security.svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Dialog from '$lib/components/ui/dialog';

  // App-wide: the "open anyway" confirmation for a refused keyless network bind (every start
  // path emits `network-exposure-blocked`) and the unverified runtime archive confirmation.

  let busy = $state(false);
  let error = $state<string | null>(null);

  const serverName = (id: string) => app.serverLabel(id);

  onMount(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    listen<ExposureBlocked>('network-exposure-blocked', (event) => {
      error = null;
      security.blocked = event.payload;
      app.refreshProfiles();
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  async function dismiss() {
    const blocked = security.blocked;
    security.blocked = null;
    if (blocked) {
      try {
        await invoke('discard_pending_exposure', { serverId: blocked.serverId });
      } catch {
        // nothing pending
      }
    }
  }

  async function openAnyway() {
    const blocked = security.blocked;
    if (!blocked || busy) return;
    busy = true;
    error = null;
    try {
      const updated = await invoke<AppSettings>('acknowledge_lan_exposure', {
        serverId: blocked.serverId,
        host: blocked.host,
        retry: true,
      });
      app.setSettings(updated);
      security.blocked = null;
    } catch (e) {
      error = t('sec.dialog_failed', { error: localizeError(e) });
      toast.error(error);
    } finally {
      busy = false;
      await app.refreshServer();
    }
  }

  function goToSettings() {
    dismiss();
    security.openSettings('security');
  }
</script>

<Dialog.Root
  open={security.blocked !== null}
  onOpenChange={(open) => {
    if (!open && !busy) dismiss();
  }}
>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        <ShieldAlert class="size-4 text-warning" />
        {t('sec.dialog_title')}
      </Dialog.Title>
      <Dialog.Description>
        {#if security.blocked}
          {t('sec.dialog_body', { server: serverName(security.blocked.serverId), host: security.blocked.host })}
        {/if}
      </Dialog.Description>
    </Dialog.Header>
    <p class="text-xs text-muted-foreground">{t('sec.dialog_hint')}</p>
    {#if error}<p class="text-xs break-words text-destructive" role="alert">{error}</p>{/if}
    <Dialog.Footer class="gap-2">
      <Button variant="outline" onclick={dismiss} disabled={busy}>{t('sec.dialog_cancel')}</Button>
      <Button variant="secondary" onclick={goToSettings} disabled={busy}>{t('sec.dialog_settings')}</Button>
      <Button variant="destructive" onclick={openAnyway} disabled={busy}>{t('sec.dialog_open_anyway')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root
  open={security.integrity !== null}
  onOpenChange={(open) => {
    if (!open) security.settleIntegrity(false);
  }}
>
  <Dialog.Content class="sm:max-w-xl">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        <FileWarning class="size-4 text-warning" />
        {t('sec.integrity_title')}
        {#if security.integrity}<span class="font-num text-xs text-muted-foreground">{security.integrity.tag}</span>{/if}
      </Dialog.Title>
      <Dialog.Description>{t('sec.integrity_body')}</Dialog.Description>
    </Dialog.Header>
    {#if security.integrity}
      <ul class="flex flex-col gap-2">
        {#each security.integrity.assets as a (a.download_url)}
          <li class="flex flex-col gap-0.5 rounded-md border border-border bg-muted/30 px-3 py-2">
            <div class="flex items-center justify-between gap-3 text-xs">
              <span class="min-w-0 truncate font-medium" title={a.name}>{a.name}</span>
              <span class="shrink-0 text-muted-foreground tabular-nums">
                {formatBytes(a.size)} ·
                {a.integrity === 'invalid' ? t('sec.integrity_reason_invalid') : t('sec.integrity_reason_missing')}
              </span>
            </div>
            <span class="font-mono text-xs break-all text-muted-foreground">{a.download_url}</span>
          </li>
        {/each}
      </ul>
    {/if}
    <Dialog.Footer class="gap-2">
      <Button variant="outline" onclick={() => security.settleIntegrity(false)}>{t('sec.integrity_cancel')}</Button>
      <Button variant="destructive" onclick={() => security.settleIntegrity(true)}>{t('sec.integrity_install')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
