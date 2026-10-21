<script lang="ts">
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { t } from '#lib/i18n.svelte.js';
  import { connections } from '../connections.svelte.ts';
  import { HarnessError } from '../harness.ts';
  import { PROVIDER_KINDS, defaultBaseUrl, providerLabel, type ProviderKind } from '../providers.ts';
  import { errorText, usd } from '../text.ts';

  let adding = $state<'harness' | 'provider' | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let detail = $state<string | null>(null);

  let name = $state('');
  let gateway = $state('');
  let api = $state('');
  let token = $state('');
  let kind = $state<ProviderKind>('openai');
  let baseUrl = $state('');
  let key = $state('');

  $effect(() => {
    for (const c of connections.harness) if (!(c.id in connections.identities)) void connections.refreshIdentity(c);
    for (const p of connections.providers) if (!connections.status[p.id]) void connections.refreshProvider(p);
  });

  function reset() {
    adding = null;
    name = gateway = api = token = baseUrl = key = '';
    error = detail = null;
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    error = detail = null;
    try {
      if (adding === 'harness') await connections.addHarness(name, gateway, api, token);
      else await connections.addProvider(kind, name, baseUrl, key);
      reset();
    } catch (err) {
      error = err instanceof HarnessError ? err.key : typeof err === 'string' ? err : 'mobile.transport';
      detail = err instanceof HarnessError ? err.detail : null;
    } finally {
      // The token/key leaves the page's memory either way.
      token = key = '';
      busy = false;
    }
  }

  const stateVariant = (s: string) => (s === 'online' ? 'secondary' : s === 'checking' ? 'outline' : 'destructive');
</script>

<div class="space-y-6 p-4">
  <section class="space-y-2">
    <h2 class="text-sm font-semibold">{t('mobile.conn.harness')}</h2>
    <p class="text-xs text-muted-foreground">{t('mobile.conn.harness_hint')}</p>
    {#each connections.harness as c (c.id)}
      {@const identity = connections.identities[c.id]}
      <div class="rounded-lg border border-border bg-card p-3" data-testid="harness-connection">
        <div class="flex items-center justify-between gap-2">
          <div class="min-w-0">
            <div class="truncate font-medium">{c.name}</div>
            <div class="truncate text-xs text-muted-foreground">{c.gatewayUrl}{c.apiUrl ? ` · ${c.apiUrl}` : ''}</div>
          </div>
          <Button variant="ghost" size="sm" onclick={() => connections.removeHarness(c.id)}>{t('mobile.remove')}</Button>
        </div>
        {#if identity}
          <div class="mt-2 flex flex-wrap gap-1">
            <Badge variant="outline">{identity.tokenName ?? t('mobile.conn.client_token')}</Badge>
            {#each identity.scopes as scope (scope)}<Badge variant="secondary">{scope}</Badge>{/each}
            {#if identity.version}<Badge variant="outline">Harness {identity.version}</Badge>{/if}
          </div>
        {:else if connections.identityErrors[c.id]}
          <p class="mt-2 text-xs text-destructive">{errorText(connections.identityErrors[c.id])}</p>
        {/if}
      </div>
    {/each}
    {#if adding !== 'harness'}
      <Button variant="outline" class="w-full" onclick={() => { reset(); adding = 'harness'; }}>{t('mobile.conn.add_harness')}</Button>
    {/if}
  </section>

  <section class="space-y-2">
    <h2 class="text-sm font-semibold">{t('mobile.conn.providers')}</h2>
    <p class="text-xs text-muted-foreground">{t('mobile.conn.providers_hint')}</p>
    {#each connections.providers as p (p.id)}
      {@const status = connections.status[p.id]}
      <div class="rounded-lg border border-border bg-card p-3" data-testid="provider-connection">
        <div class="flex items-center justify-between gap-2">
          <div class="min-w-0">
            <div class="truncate font-medium">{p.name} <span class="text-xs text-muted-foreground">· {providerLabel(p.kind)}</span></div>
            <div class="truncate text-xs text-muted-foreground">{p.baseUrl}</div>
          </div>
          <Button variant="ghost" size="sm" onclick={() => connections.removeProvider(p.id)}>{t('mobile.remove')}</Button>
        </div>
        {#if status}
          <div class="mt-2 flex flex-wrap items-center gap-1 text-xs">
            <Badge variant={stateVariant(status.state)}>{t(`mobile.state.${status.state}`)}</Badge>
            {#if status.state === 'online'}<span class="text-muted-foreground">{t('mobile.conn.models', { n: status.models.length })}</span>{/if}
            {#if status.quota}<span class="text-muted-foreground">{t('mobile.conn.credits', { left: usd(status.quota.remaining), used: usd(status.quota.usage) })}</span>{/if}
            <Button variant="ghost" size="xs" onclick={() => connections.refreshProvider(p)}>{t('mobile.refresh')}</Button>
          </div>
          {#if status.error && status.state !== 'online'}
            <p class="mt-1 text-xs text-destructive">{errorText(status.error)}{status.detail ? ` — ${status.detail}` : ''}</p>
          {/if}
        {/if}
      </div>
    {/each}
    {#if adding !== 'provider'}
      <Button variant="outline" class="w-full" onclick={() => { reset(); adding = 'provider'; }}>{t('mobile.conn.add_provider')}</Button>
    {/if}
  </section>

  {#if adding}
    <form class="space-y-3 rounded-lg border border-border bg-card p-3" onsubmit={submit}>
      <h3 class="text-sm font-semibold">{adding === 'harness' ? t('mobile.conn.add_harness') : t('mobile.conn.add_provider')}</h3>
      {#if adding === 'provider'}
        <div class="space-y-1">
          <Label for="kind">{t('mobile.conn.kind')}</Label>
          <select id="kind" class="h-9 w-full rounded-md border border-input bg-background px-2 text-sm" bind:value={kind}>
            {#each PROVIDER_KINDS as k (k)}<option value={k}>{providerLabel(k)}</option>{/each}
          </select>
        </div>
      {/if}
      <div class="space-y-1">
        <Label for="name">{t('mobile.conn.name')}</Label>
        <Input id="name" bind:value={name} autocomplete="off" />
      </div>
      {#if adding === 'harness'}
        <div class="space-y-1">
          <Label for="gateway">{t('mobile.conn.gateway_url')}</Label>
          <Input id="gateway" bind:value={gateway} placeholder="http://192.168.1.20:11435" inputmode="url" autocapitalize="off" autocomplete="off" required />
        </div>
        <div class="space-y-1">
          <Label for="api">{t('mobile.conn.api_url')}</Label>
          <Input id="api" bind:value={api} placeholder="http://192.168.1.20:11500" inputmode="url" autocapitalize="off" autocomplete="off" />
          <p class="text-xs text-muted-foreground">{t('mobile.conn.api_hint')}</p>
        </div>
        <div class="space-y-1">
          <Label for="token">{t('mobile.conn.token')}</Label>
          <Input id="token" type="password" bind:value={token} autocapitalize="off" autocomplete="off" required />
          <p class="text-xs text-muted-foreground">{t('mobile.conn.token_hint')}</p>
        </div>
      {:else}
        <div class="space-y-1">
          <Label for="base">{t('mobile.conn.base_url')}</Label>
          <Input id="base" bind:value={baseUrl} placeholder={defaultBaseUrl(kind) ?? 'https://…/v1'} inputmode="url" autocapitalize="off" autocomplete="off" required={kind === 'custom'} />
        </div>
        <div class="space-y-1">
          <Label for="key">{t('mobile.conn.key')}</Label>
          <Input id="key" type="password" bind:value={key} autocapitalize="off" autocomplete="off" required={kind !== 'custom'} />
          <p class="text-xs text-muted-foreground">{t('mobile.conn.key_hint')}</p>
        </div>
      {/if}
      {#if error}<p class="text-xs text-destructive" role="alert">{errorText(error)}{detail ? ` — ${detail}` : ''}</p>{/if}
      <div class="flex gap-2">
        <Button type="submit" disabled={busy} class="flex-1">{busy ? t('mobile.checking') : t('mobile.connect')}</Button>
        <Button type="button" variant="ghost" onclick={reset}>{t('mobile.cancel')}</Button>
      </div>
    </form>
  {/if}
</div>
