<script lang="ts">
  // Connections > Ollama-compatible endpoint (P2): the gateway address for Ollama clients
  // (OLLAMA_HOST) and, when turned on, the extra listener on the Ollama port.
  import { Boxes, Settings2, TriangleAlert } from '@lucide/svelte';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import { t } from '$lib/i18n.svelte';
  import { localizeError } from '$lib/network';
  import { app } from '$lib/stores/app.svelte';
  import { security } from '$lib/stores/security.svelte';
  import { cn } from '$lib/utils';
  import CopyButton from './CopyButton.svelte';

  let gw = $derived(app.gateway);
  let on = $derived(app.gatewayRunning);
  let address = $derived(on ? (gw?.status.endpoint?.host_url ?? '') : '');
  let hostPort = $derived(address.replace(/^https?:\/\//, ''));
  let extraPort = $derived(on ? (gw?.status.ollama_port ?? null) : null);
  let warning = $derived(on ? (gw?.status.ollama_warning ?? null) : null);
</script>

<div class={cn('flex flex-col gap-1.5 rounded-lg border p-3', on ? 'border-border' : 'border-dashed border-border')}>
  <div class="flex items-center justify-between gap-2">
    <span class={cn('flex items-center gap-1.5 text-sm font-medium', !on && 'text-muted-foreground')}>
      <Boxes class="size-3.5" />
      {t('ollama.card_title')}
    </span>
    {#if extraPort}
      <Badge variant="outline" class="font-num">{t('ollama.card_port_badge', { port: extraPort })}</Badge>
    {/if}
  </div>
  <p class="text-xs text-muted-foreground">{on ? t('ollama.card_desc') : t('ollama.card_off')}</p>
  {#if on && address}
    <div class="flex flex-col gap-1 text-xs">
      <div class="flex items-center gap-2">
        <span class="w-28 shrink-0 text-muted-foreground">{t('ollama.address')}</span>
        <span class="truncate font-mono text-foreground" title={address}>{address}</span>
        <CopyButton text={address} ariaLabel={t('conn.copy_url_aria', { name: t('ollama.card_title') })} />
      </div>
      <div class="flex items-center gap-2">
        <span class="w-28 shrink-0 text-muted-foreground">{t('ollama.card_env')}</span>
        <span class="truncate font-mono text-foreground">OLLAMA_HOST={hostPort}</span>
        <CopyButton text={`OLLAMA_HOST=${hostPort}`} ariaLabel={t('conn.copy_url_aria', { name: 'OLLAMA_HOST' })} />
      </div>
    </div>
  {/if}
  {#if warning}
    <p class="flex items-start gap-1.5 text-xs text-warning" role="alert">
      <TriangleAlert class="mt-0.5 size-3.5 shrink-0" />
      <span>{localizeError(warning)}</span>
    </p>
  {/if}
  <div>
    <Button variant="ghost" size="xs" onclick={() => security.openSettings('gateway')}>
      <Settings2 />
      {t('gw.ep_open_settings')}
    </Button>
  </div>
</div>
