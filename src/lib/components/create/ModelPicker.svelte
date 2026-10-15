<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import { app } from '#lib/stores/app.svelte.js';
  import { domain } from '#lib/stores/domain.svelte.js';
  import { library } from '#lib/stores/library.svelte.js';
  import { chatModelIds } from '../playground/modelModes';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { invoke } from '#lib/ipc.js';
  import { validModelEndpoint } from '#lib/gateway-error.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import FieldHelp from '../common/FieldHelp.svelte';
  import { fetchErrorText, fetchWithTimeout } from '../connections/net';
  import type { GenerateToolRequest } from '#lib/tools-client.js';
  import { security } from '#lib/stores/security.svelte.js';
  let { endpoint = $bindable(null), disabled = false, selectedModel = $bindable('') }: { endpoint: GenerateToolRequest['endpoint'] | null; disabled?: boolean; selectedModel?: string } = $props();
  const uid = $props.id();
  let url = $state('');
  let useGateway = $state(true);
  let models = $state<string[]>([]);
  let fits = $state<Record<string, string>>({});
  let error = $state('');
  let loading = $state(false);
  let userPicked = $state(false);
  let extraHeaders = $state<Record<string, string>>({});
  let controller: AbortController | null = null;
  let valid = $derived(validModelEndpoint(url));
  let gatewayOff = $derived(useGateway && !app.gatewayRunning);
  const headers = () => useGateway ? app.inferenceHeaders() : extraHeaders;
  function port(alias: string) {
    const def = domain.definitions.find(d => d.alias === alias);
    return def ? domain.state(def.id)?.servers.find(s => s.status === 'Running')?.port : undefined;
  }
  async function load() {
    controller?.abort(); const current = new AbortController(); controller = current;
    endpoint = null; error = ''; loading = true; models = []; fits = {};
    const base = useGateway ? app.gatewayBaseUrl : url;
    const requestUrl = (base ?? '').replace(/\/$/, '') + '/models';
    try {
      if (!base || !validModelEndpoint(base)) return;
      await Promise.all([domain.refreshDefinitions(), domain.refreshStates(), library.refresh().catch(() => [])]);
      if (current.signal.aborted || (useGateway && base !== app.gatewayBaseUrl)) return;
      const r = await fetchWithTimeout(requestUrl, { headers: headers(), signal: current.signal }, 10000);
      if (!r.ok) throw new Error('HTTP ' + r.status);
      const data = await r.json();
      if (current.signal.aborted) return;
      models = chatModelIds((data.data ?? []).map((m: { id: string }) => m.id), domain.definitions, library.models);
      selectedModel = userPicked && models.includes(selectedModel) ? selectedModel : models.find(m => port(m)) ?? models[0] ?? '';
      const estimates = await invoke<{ alias: string; status: string }[]>('model_picker_memory');
      if (!current.signal.aborted) fits = Object.fromEntries(estimates.map(e => [e.alias, e.status]));
    } catch (e) { if (!current.signal.aborted) error = t('create.models_error', { error: fetchErrorText(e, requestUrl) }); }
    finally { if (controller === current) loading = false; }
  }
  async function choose(base: string, serverId?: string) {
    useGateway = false;
    url = base; selectedModel = ''; userPicked = false; extraHeaders = {};
    if (serverId) { const key = await invoke<string | null>('get_server_api_key', { serverId }); if (key) extraHeaders = { Authorization: `Bearer ${key}` }; }
    await load();
  }
  $effect(() => { endpoint = !gatewayOff && valid && selectedModel && models.includes(selectedModel) ? { base_url: url.replace(/\/$/, ''), headers: headers(), model: selectedModel } : null; });
  $effect(() => {
    if (!useGateway) return;
    const base = app.gatewayBaseUrl;
    untrack(() => {
      url = base ?? '';
      selectedModel = ''; userPicked = false;
      void load();
    });
  });
  let serverSignature = $derived(app.servers.map(s => s.id + ':' + JSON.stringify(s.status)).join('|'));
  $effect(() => { void serverSignature; void domain.refreshStates(); });
  onDestroy(() => controller?.abort());
</script>
<div class="flex flex-wrap items-end gap-3">
  <label class="flex min-w-0 flex-1 flex-col gap-1 text-xs text-muted-foreground">{t('create.endpoint_base')}
    <Input aria-describedby={uid + '-endpoint-help'} aria-invalid={!gatewayOff && !valid} class="font-mono text-xs" bind:value={url} {disabled} oninput={() => { useGateway = false; controller?.abort(); models = []; selectedModel = ''; extraHeaders = {}; }} />
  </label>
  <label class="flex min-w-40 flex-1 flex-col gap-1 text-xs text-muted-foreground">{t('create.model')}
    <select onchange={() => userPicked = true} aria-describedby={uid + '-model-help'} class="h-8 max-w-full rounded-md border border-input bg-background px-2 text-sm text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring" bind:value={selectedModel} disabled={disabled || loading}>
      <option value="">{t('create.model')}</option>{#each models as m}<option value={m}>{m} · {port(m) ? t('create.model_running', { port: port(m)! }) : t(('create.model_' + (fits[m] ?? 'unknown')) as TKey)}</option>{/each}
    </select>
  </label>
  <Button variant="outline" disabled={disabled || loading || !valid || gatewayOff} onclick={load}>{t('create.refresh')}</Button>
</div>
<FieldHelp id={uid + '-endpoint-help'} text={t('create.endpoint_help')} />
{#if gatewayOff}
  <div class="flex items-center gap-2 text-xs text-muted-foreground" role="status">
    <span>{t('gw.off')}</span>
    <Button variant="outline" size="sm" {disabled} onclick={() => security.openSettings('gateway-endpoint')}>{t('gw.ep_open_settings')}</Button>
  </div>
{:else if !valid}<p class="text-xs text-destructive" role="alert">{t('create.endpoint_invalid')}</p>{/if}
<div class="flex flex-wrap gap-2">
  <Button variant="outline" size="sm" {disabled} onclick={() => { if (useGateway) void load(); else useGateway = true; }}>{t('create.gateway_option')}</Button>
  {#each app.servers.filter(s => s.status === 'Running' && s.active) as server}
    <Button variant="outline" size="sm" {disabled} onclick={() => choose(`http://${server.active!.connect_host}:${server.active!.port}/v1`, server.id)}>{app.serverLabel(server.id)} :{server.active!.port}</Button>
  {/each}
</div>
<FieldHelp id={uid + '-model-help'} text={t('create.model_help')} />
{#if error}<p class="text-xs text-warning" role="alert">{error}</p>{/if}
