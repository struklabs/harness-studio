<script lang="ts">
  import { t } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { gatewayMemoryError, gatewayBusyError } from '#lib/gateway-error.js';
  import { busyText } from '#lib/gateway-busy-text.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { domain } from '#lib/stores/domain.svelte.js';
  import { invoke } from '#lib/ipc.js';
  import { Button } from '#lib/components/ui/button/index.js';
  let { error, onUse, onRetry }: { error: unknown; onUse?: (model: string) => void; onRetry?: () => void | Promise<unknown> } = $props();
  let busy = $state(false);
  let actionError = $state('');
  let memory = $derived(gatewayMemoryError(error));
  let busyError = $derived(gatewayBusyError(error));
  let blockers = $derived((memory?.blockers ?? []).flatMap(alias => {
    const def = domain.definitions.find(d => d.alias.toLowerCase() === alias.toLowerCase());
    const servers = def ? domain.state(def.id)?.servers ?? [] : [];
    return servers.filter(s => s.status === 'Running').map(s => ({ ...s, alias }));
  }));
  $effect(() => { if (memory) { void domain.refreshDefinitions(); void domain.refreshStates(); } });
  async function stop(serverId: string) {
    busy = true; actionError = '';
    try {
      await invoke('stop_server', { serverId });
      await domain.refreshStates(); await app.refreshGateway(true);
      // Admission caches device usage briefly. After an explicit unload refresh it before retrying.
      await invoke('get_runtime_devices', { refresh: true });
      await onRetry?.();
    }
    catch (e) { actionError = localizeError(e); }
    finally { busy = false; }
  }
</script>
{#if memory}
  <div class="space-y-2 rounded-md border border-warning/40 p-3 text-xs" role="alert">
    <p>{t('create.memory_blocked', { model: memory.model, blockers: memory.blockers.join(', ') || t('create.memory_other') })}</p>
    <div class="flex flex-wrap gap-2">
      {#each blockers as blocker}
        {#if onUse}<Button variant="outline" disabled={busy} onclick={() => onUse?.(blocker.alias)}>{t('create.use_loaded', { name: blocker.alias })}</Button>{/if}
        {#if onRetry}<Button variant="outline" disabled={busy} onclick={() => stop(blocker.server_id)}>{t('create.stop_load', { server: app.serverLabel(blocker.server_id), model: memory.model })}</Button>{/if}
      {/each}
    </div>
    {#if actionError}<p class="text-destructive">{actionError}</p>{/if}
    <details><summary class="cursor-pointer">{t('create.error_details')}</summary><pre class="mt-2 overflow-auto whitespace-pre-wrap break-words">{memory.raw}</pre></details>
  </div>
{:else if busyError}
  <div class="space-y-2 rounded-md border border-warning/40 p-3 text-xs" role="alert">
    <p>{busyText(busyError)}</p>
    {#if onRetry}<Button variant="outline" onclick={() => onRetry?.()}>{t('pg.retry')}</Button>{/if}
    <details><summary class="cursor-pointer">{t('create.error_details')}</summary><pre class="mt-2 overflow-auto whitespace-pre-wrap break-words">{busyError.raw}</pre></details>
  </div>
{:else if error}<p class="text-xs text-destructive" role="alert">{localizeError(error)}</p>{/if}
