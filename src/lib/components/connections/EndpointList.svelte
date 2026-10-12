<script lang="ts">
  import { uiLevel } from '$lib/stores/uiLevel.svelte';
  import SettingRow from '../settings/SettingRow.svelte';
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import { onMount } from 'svelte';
  import { invoke } from '$lib/ipc';
  import { Lock, LockOpen, Network, Route, Settings2, ShieldAlert } from '@lucide/svelte';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import * as Table from '$lib/components/ui/table';
  import { t } from '$lib/i18n.svelte';
  import { bindScope } from '$lib/network';
  import { app } from '$lib/stores/app.svelte';
  import { security } from '$lib/stores/security.svelte';
  import type { LanAddress, NetworkOverview } from '$lib/types';
  import { cn } from '$lib/utils';
  import CopyButton from './CopyButton.svelte';

  let baseUrl = $derived(app.baseUrl);
  let hostUrl = $derived(app.hostUrl);

  let rows = $derived([
    { method: 'GET · POST', label: t('conn.ep_base_url_label'), url: baseUrl },
    { method: 'POST', label: t('conn.ep_chat_label'), url: `${baseUrl}/chat/completions` },
    { method: 'POST', label: t('conn.ep_anthropic_label'), url: `${baseUrl}/messages` },
    { method: 'POST', label: t('conn.ep_embeddings_label'), url: `${baseUrl}/embeddings` },
    { method: 'GET', label: t('conn.ep_metrics_label'), url: `${hostUrl}/metrics` },
  ]);

  // Managed endpoint (gateway): one address for every model definition; the default for
  // Harness's own clients while it runs.
  let gw = $derived(app.gateway);
  let managedOn = $derived(app.gatewayRunning);
  let managedBase = $derived(gw?.status.endpoint?.base_url ?? '');
  let managedRows = $derived(
    managedOn
      ? [
          { method: 'GET · POST', label: t('conn.ep_base_url_label'), url: managedBase },
          { method: 'GET', label: t('gw.ep_models_label'), url: `${managedBase}/models` },
          { method: 'POST', label: t('conn.ep_chat_label'), url: `${managedBase}/chat/completions` },
          { method: 'POST', label: 'Responses (Codex)', url: `${managedBase}/responses` },
          { method: 'POST', label: t('conn.ep_anthropic_label'), url: `${managedBase}/messages` },
          { method: 'POST', label: t('conn.ep_embeddings_label'), url: `${managedBase}/embeddings` },
          { method: 'POST', label: 'Rerank', url: `${managedBase}/rerank` },
        ]
      : [],
  );

  // Bind scope / auth of the selected server: the live run, else its saved configuration.
  let bindHost = $derived(
    app.activeServer?.bind_host ?? (app.selectedProfile?.config.host || app.settings?.server.host || '127.0.0.1'),
  );
  let scope = $derived(bindScope(bindHost));
  let keySet = $derived(app.activeServer ? app.activeServer.api_key_set : !!app.selectedProfile?.config.api_key_ref);
  let exposedWithoutKey = $derived(scope === 'lan' && !keySet && !!app.activeServer);

  let lanAddresses = $state<LanAddress[]>([]);
  // A wildcard bind is reachable on every interface; a specific address only on itself.
  let lanUrls = $derived(
    scope !== 'lan' || app.portPending
      ? []
      : bindHost === '0.0.0.0' || bindHost === '::'
        ? lanAddresses.map((a) => `http://${a.ip}:${app.port}/v1`)
        : [`http://${bindHost.includes(':') ? `[${bindHost}]` : bindHost}:${app.port}/v1`],
  );

  onMount(async () => {
    void app.refreshGateway(true);
    try {
      lanAddresses = (await invoke<NetworkOverview>('get_network_overview')).lan_addresses;
    } catch {
      lanAddresses = [];
    }
  });
</script>

{#snippet urlTable(list: { method: string; label: string; url: string }[])}
  <div class="overflow-hidden rounded-lg border border-border bg-card">
    <Table.Root class="table-fixed">
      <Table.Header>
        <Table.Row class="hover:bg-transparent">
          <Table.Head class="w-28 text-xs">{t('conn.col_method')}</Table.Head>
          <Table.Head class="w-[30%] text-xs">{t('conn.col_endpoint')}</Table.Head>
          <Table.Head class="text-xs">{t('conn.col_url')}</Table.Head>
          <Table.Head class="w-10"><span class="sr-only">{t('conn.copy')}</span></Table.Head>
        </Table.Row>
      </Table.Header>
      <Table.Body>
        {#each list as row (row.url)}
          <Table.Row>
            <Table.Cell>
              <span class="font-mono text-xs whitespace-nowrap text-muted-foreground">{row.method}</span>
            </Table.Cell>
            <Table.Cell class="truncate text-xs">{row.label}</Table.Cell>
            <Table.Cell class="truncate font-mono text-xs text-foreground" title={row.url}>{row.url}</Table.Cell>
            <Table.Cell class="text-right">
              <CopyButton text={row.url} ariaLabel={t('conn.copy_url_aria', { name: row.label })} />
            </Table.Cell>
          </Table.Row>
        {/each}
      </Table.Body>
    </Table.Root>
  </div>
{/snippet}

<section class="flex min-w-0 flex-col gap-3" aria-labelledby="conn-endpoints-title">
  <div class="flex items-center justify-between gap-3">
    <h2 id="conn-endpoints-title" class="text-base font-medium">{t('conn.endpoints')}</h2>
    <AdvancedOnly><Badge variant="outline" class="text-muted-foreground">{t('conn.badge_compatible')}</Badge></AdvancedOnly>
  </div>

  {#if !uiLevel.advanced}
    <SettingRow label={t('conn.ep_base_url_label')} hint={t('conn.simple_endpoint_hint')} note={!managedOn ? t('gw.ep_managed_off') : undefined}>
      {#if managedOn && managedBase}
        <code class="min-w-0 max-w-full break-all font-mono text-xs">{managedBase}</code>
        <CopyButton text={managedBase} ariaLabel={t('conn.copy_url_aria', { name: t('conn.ep_base_url_label') })} />
      {/if}
      <Button variant="outline" size="sm" onclick={() => security.openSettings('gateway')}><Settings2 />{t('gw.ep_open_settings')}</Button>
    </SettingRow>
  {/if}

  <AdvancedOnly>
    <!-- Endpoint kinds: managed (gateway) and direct llama-server. -->
    <div class="grid grid-cols-2 gap-2 max-[1100px]:grid-cols-1">
      <div
        class={cn(
          'flex flex-col gap-1.5 rounded-lg border p-3',
          managedOn ? 'border-border bg-card' : 'border-dashed border-border',
        )}
      >
        <div class="flex items-center justify-between gap-2">
          <span class={cn('flex items-center gap-1.5 text-sm font-medium', !managedOn && 'text-muted-foreground')}>
            <Route class="size-3.5" />
            {t('sec.ep_managed_title')}
          </span>
          {#if managedOn}
            <Badge variant="outline" class="text-success">{t('gw.ep_default')}</Badge>
          {:else}
            <Badge variant="outline" class="text-muted-foreground">{t('gw.status_stopped')}</Badge>
          {/if}
        </div>
        <p class="text-xs text-muted-foreground">{managedOn ? t('gw.ep_managed_desc') : t('gw.ep_managed_off')}</p>
        <div class="flex flex-wrap items-center gap-1.5">
          {#if managedOn && gw}
            <Badge variant="outline" title={gw.status.host ?? ''}>
              {bindScope(gw.status.host ?? '127.0.0.1') === 'lan' ? t('sec.scope_lan') : t('sec.scope_loopback')} ·
              <span class="font-num">{gw.status.host}:{gw.status.port}</span>
            </Badge>
            <Badge variant="outline" class={cn(gw.status.require_token && 'text-success')}>
              {#if gw.status.require_token}<Lock class="size-3" />{:else}<LockOpen class="size-3" />{/if}
              {gw.status.require_token ? t('gw.ep_token_required') : t('gw.ep_token_open')}
            </Badge>
          {/if}
          <Button variant="ghost" size="xs" onclick={() => security.openSettings('gateway')}>
            <Settings2 />
            {t('gw.ep_open_settings')}
          </Button>
        </div>
      </div>
      <div
        class={cn('flex flex-col gap-1.5 rounded-lg border p-3', 'border-border')}
      >
        <div class="flex items-center justify-between gap-2">
          <span class="flex items-center gap-1.5 text-sm font-medium">
            <Network class="size-3.5" />
            {t('sec.ep_direct_title')}
          </span>
          {#if !managedOn}<Badge variant="outline" class="text-success">{t('sec.ep_active')}</Badge>{/if}
        </div>
        <p class="text-xs text-muted-foreground">{managedOn ? t('gw.ep_direct_note') : t('sec.ep_direct_desc')}</p>
        <div class="flex flex-wrap items-center gap-1.5">
          <Badge variant="outline" class={cn(scope === 'lan' && 'text-warning')} title={bindHost}>
            {scope === 'lan' ? t('sec.scope_lan') : t('sec.scope_loopback')} · <span class="font-num">{bindHost}</span>
          </Badge>
          <Badge variant="outline" class={cn(keySet ? 'text-success' : scope === 'lan' && 'text-destructive')}>
            {#if keySet}<Lock class="size-3" />{:else}<LockOpen class="size-3" />{/if}
            {keySet ? t('sec.auth_key') : t('sec.auth_none')}
          </Badge>
        </div>
      </div>
    </div>
  </AdvancedOnly>


  {#if exposedWithoutKey}
    <p class="flex items-start gap-1.5 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs" role="alert">
      <ShieldAlert class="mt-0.5 size-3.5 shrink-0 text-warning" />
      <span>
        <span class="font-medium">{t('sec.banner_unprotected')}</span> ·
        {t('sec.banner_unprotected_desc', { servers: app.serverLabel(app.selectedServerId) })}
      </span>
    </p>
  {/if}

  <AdvancedOnly>
    {#if managedOn}
      <span class="text-xs font-medium">{t('gw.ep_managed_rows')}</span>
      {@render urlTable(managedRows)}
      <span class="text-xs font-medium">{t('gw.ep_direct_rows', { server: app.serverLabel(app.selectedServerId) })}</span>
    {/if}
    {#if app.portPending}
      <p class="text-xs text-muted-foreground">{t('conn.ep_port_auto')}</p>
    {:else}
      {@render urlTable(rows)}
    {/if}

    {#if lanUrls.length > 0}
      <div class="flex flex-col gap-1.5 text-xs">
        <span class="text-muted-foreground">{t('sec.ep_lan_urls')}</span>
        {#each lanUrls as url (url)}
          <div class="flex items-center gap-2">
            <span class="truncate font-mono text-foreground" title={url}>{url}</span>
            <CopyButton text={url} ariaLabel={t('conn.copy_url_aria', { name: url })} />
          </div>
        {/each}
        <p class="text-muted-foreground">{t('sec.firewall_note')}</p>
      </div>
    {/if}

    <p class="text-xs text-muted-foreground">
      {#if app.apiKey}
        {t('conn.ep_auth_key')}
      {:else}
        {t('conn.ep_auth_none')}
      {/if}
    </p>
  </AdvancedOnly>
</section>
