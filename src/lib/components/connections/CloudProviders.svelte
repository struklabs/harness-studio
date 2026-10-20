<script lang="ts">
  // BYOK cloud providers: add a provider with its API key (kept in the system keychain), check
  // it, see its models and what the API says about them, try one model, remove the key.
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { t } from '#lib/i18n.svelte.js';
  import { errorText } from '#lib/errors.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { TriangleAlert } from '@lucide/svelte';
  import { filterCloudModels, formatPrice, type CloudModel } from '../playground/cloudModels';

  type Kind = CloudModel['kind'];
  interface Provider {
    id: string;
    name: string;
    kind: Kind;
    label: string;
    base_url: string;
    enabled: boolean;
    key_set: boolean;
    state: CloudModel['state'];
    model_count: number;
    last_error: string | null;
    last_ok: number | null;
    latency_ms: number | null;
    quota: { limit: number | null; usage: number | null; remaining: number | null } | null;
  }
  interface ModelTest { ok: boolean; latency_ms: number; tool_called: boolean; model: string | null; detail: string }

  const KINDS: { kind: Kind; label: string; url: string }[] = [
    { kind: 'openai', label: 'OpenAI', url: 'https://api.openai.com/v1' },
    { kind: 'gemini', label: 'Gemini', url: 'https://generativelanguage.googleapis.com/v1beta/openai' },
    { kind: 'anthropic', label: 'Anthropic', url: 'https://api.anthropic.com/v1' },
    { kind: 'openrouter', label: 'OpenRouter', url: 'https://openrouter.ai/api/v1' },
    { kind: 'custom', label: '', url: 'http://localhost:8000/v1' },
  ];

  let providers = $state<Provider[]>([]);
  let models = $state<CloudModel[]>([]);
  let busy = $state(false);
  let error = $state('');
  let notice = $state('');
  let open = $state(false);
  let editing = $state<Provider | null>(null);
  let removing = $state<Provider | null>(null);
  let kind = $state<Kind>('openai');
  let name = $state('');
  let address = $state('');
  let key = $state('');
  let removeKey = $state(false);
  let enabled = $state(true);
  let formError = $state('');
  let expanded = $state<Record<string, boolean>>({});
  let filters = $state<Record<string, string>>({});
  let testModel = $state<Record<string, string>>({});
  let modelResults = $state<Record<string, ModelTest | string>>({});
  let alive = true;

  const kindInfo = $derived(KINDS.find((k) => k.kind === kind)!);
  const addressChanged = $derived.by(() => {
    if (!editing) return false;
    try { return new URL(address.trim() || kindInfo.url).origin !== new URL(editing.base_url).origin; }
    catch { return false; }
  });
  const needsKey = $derived(kind !== 'custom' && (!editing || !editing.key_set || addressChanged) && !removeKey);

  function kindLabel(k: Kind): string { return k === 'custom' ? t('cloud.kind_custom') : KINDS.find((i) => i.kind === k)!.label; }
  function stateLabel(state: Provider['state']): string {
    return t(state === 'online' ? 'cloud.online' : state === 'unauthorized' ? 'cloud.unauthorized_state' : state === 'key_missing' ? 'cloud.key_missing_state' : 'cloud.offline');
  }
  function usd(v: number | null): string { return v === null ? t('cloud.quota_unlimited') : `${Math.round(v * 100) / 100} USD`; }

  async function load() {
    try {
      const [list, rows] = await Promise.all([invoke<Provider[]>('cloud_list'), invoke<CloudModel[]>('cloud_models')]);
      if (alive) { providers = list; models = rows; error = ''; }
    } catch (e) { if (alive) error = errorText(e); }
  }

  onMount(() => {
    void load();
    const timer = setInterval(() => { if (!busy) void load(); }, 15000);
    return () => { alive = false; clearInterval(timer); key = ''; };
  });

  function edit(provider: Provider | null) {
    editing = provider;
    kind = provider?.kind ?? 'openai';
    name = provider?.name ?? 'openai';
    address = provider && provider.base_url !== KINDS.find((k) => k.kind === provider.kind)?.url ? provider.base_url : '';
    key = '';
    removeKey = false;
    enabled = provider?.enabled ?? true;
    formError = '';
    open = true;
  }

  function pickKind(value: Kind) {
    // A fresh provider is named after its kind until the user types a name.
    if (!editing && (name === '' || KINDS.some((k) => k.kind === name))) name = value;
    kind = value;
  }

  async function save() {
    busy = true;
    formError = '';
    try {
      const input = { name, kind, base_url: address, key: removeKey ? '' : key || null, enabled };
      editing = editing
        ? await invoke<Provider>('cloud_update', { id: editing.id, input })
        : await invoke<Provider>('cloud_add', { input });
      open = false;
      await load();
    } catch (e) { formError = errorText(e); }
    finally { key = ''; busy = false; }
  }

  async function test(provider: Provider) {
    busy = true;
    error = '';
    try { await invoke('cloud_test', { id: provider.id }); await load(); }
    catch (e) { error = errorText(e); }
    finally { busy = false; }
  }

  async function tryModel(provider: Provider) {
    const model = testModel[provider.id];
    if (!model) return;
    busy = true;
    try { modelResults[provider.id] = await invoke<ModelTest>('cloud_test_model', { id: provider.id, model }); }
    catch (e) { modelResults[provider.id] = errorText(e); }
    finally { busy = false; }
  }

  async function remove() {
    if (!removing) return;
    notice = '';
    busy = true;
    try {
      notice = (await invoke<string | null>('cloud_remove', { id: removing.id })) ?? '';
      removing = null;
      await load();
    }
    catch (e) { error = errorText(e); }
    finally { busy = false; }
  }
</script>

<div class="flex flex-col gap-4">
  <p class="text-sm text-muted-foreground">{t('cloud.description')}</p>
  <p class="text-sm text-muted-foreground">{t('cloud.privacy')}</p>
  {#if !app.gateway?.status.running}
    <p class="rounded-lg border border-border p-3 text-sm text-muted-foreground">{t('cloud.gateway_off')}</p>
  {/if}
  <div><Button onclick={() => edit(null)} disabled={busy}>{t('cloud.add')}</Button></div>
  {#if error}<p class="text-sm text-destructive" role="alert">{error}</p>{/if}
  {#if notice}<p class="flex items-start gap-2 text-sm text-warning" role="status"><TriangleAlert class="size-4 shrink-0" />{errorText(notice)}</p>{/if}
  {#if providers.length === 0}<p class="text-sm text-muted-foreground">{t('cloud.empty')}</p>{/if}
  {#each providers as provider (provider.id)}
    {@const rows = models.filter((m) => m.provider === provider.name)}
    {@const result = modelResults[provider.id]}
    <article class="flex flex-col gap-3 rounded-lg border border-border bg-card p-4">
      <div class="flex flex-wrap items-center gap-2">
        <h2 class="text-sm font-semibold">{provider.name}</h2>
        <Badge variant="secondary">{kindLabel(provider.kind)}</Badge>
        <Badge variant="outline">{stateLabel(provider.state)}</Badge>
        {#if !provider.enabled}<Badge variant="outline">{t('cloud.disabled')}</Badge>{/if}
        <span class="text-xs text-muted-foreground">{t('cloud.model_count', { count: provider.model_count })}</span>
        {#if provider.latency_ms !== null}<span class="text-xs text-muted-foreground">{t('cloud.latency', { ms: provider.latency_ms })}</span>{/if}
      </div>
      <p class="break-all font-mono text-xs text-muted-foreground">{provider.base_url}</p>
      {#if provider.quota}<p class="text-xs text-muted-foreground">{t('cloud.quota', { usage: provider.quota.usage ?? 0, limit: usd(provider.quota.limit) })}</p>{/if}
      {#if provider.last_error}<p class="text-xs text-destructive">{errorText(provider.last_error)}</p>{/if}
      {#if provider.last_ok}<p class="text-xs text-muted-foreground">{t('cloud.last_ok', { time: new Date(provider.last_ok * 1000).toLocaleString() })}</p>{/if}
      <div class="flex flex-wrap gap-2">
        <Button variant="outline" size="sm" disabled={busy} onclick={() => test(provider)}>{t('cloud.test')}</Button>
        {#if rows.length > 0}<Button variant="outline" size="sm" aria-expanded={!!expanded[provider.id]} onclick={() => { expanded[provider.id] = !expanded[provider.id]; }}>{t('cloud.models')}</Button>{/if}
        <Button variant="outline" size="sm" disabled={busy} onclick={() => edit(provider)}>{t('cloud.edit')}</Button>
        <Button variant="outline" size="sm" disabled={busy} onclick={() => { removing = provider; }}>{t('cloud.remove')}</Button>
      </div>
      {#if expanded[provider.id] && rows.length > 0}
        <div class="flex flex-col gap-2 border-t border-border pt-3">
          <Input bind:value={filters[provider.id]} aria-label={t('cloud.model_filter')} placeholder={t('cloud.model_filter')} />
          <ul class="max-h-72 overflow-y-auto text-xs">
            {#each filterCloudModels(rows, filters[provider.id] ?? '') as m (m.id)}
              <li class="flex flex-col gap-0.5 border-b border-border/60 py-1.5">
                <span class="font-mono">{m.model}{#if m.display_name}<span class="ml-2 font-sans text-muted-foreground">{m.display_name}</span>{/if}</span>
                <span class="flex flex-wrap gap-x-3 text-muted-foreground">
                  {#if m.context_length}<span>{t('cloud.context', { count: m.context_length.toLocaleString() })}</span>{/if}
                  {#if m.input_price !== null && m.output_price !== null}<span>{t('cloud.price', { input: formatPrice(m.input_price), output: formatPrice(m.output_price) })}</span>{/if}
                  {#if m.tools}<span>{t('cloud.tools')}</span>{/if}
                  {#if m.vision}<span>{t('cloud.vision')}</span>{/if}
                </span>
              </li>
            {:else}
              <li class="py-1.5 text-muted-foreground">{t('cloud.models_empty')}</li>
            {/each}
          </ul>
          <div class="flex flex-wrap items-center gap-2">
            <select class="h-8 max-w-full rounded-md border border-input bg-background px-2 text-xs" aria-label={t('cloud.test_model')} bind:value={testModel[provider.id]}>
              <option value=""></option>
              {#each rows as m (m.id)}<option value={m.model}>{m.model}</option>{/each}
            </select>
            <Button variant="outline" size="sm" disabled={busy || !testModel[provider.id]} onclick={() => tryModel(provider)}>{t('cloud.test_model')}</Button>
          </div>
          <p class="text-xs text-muted-foreground">{t('cloud.test_model_hint')}</p>
          {#if typeof result === 'string'}
            <p class="text-xs text-destructive" role="alert">{result}</p>
          {:else if result}
            <p class={result.ok ? 'text-xs text-success' : 'text-xs text-destructive'} role="status">
              {result.ok ? `${t('cloud.test_ok', { model: result.model ?? testModel[provider.id], ms: result.latency_ms })} ${t(result.tool_called ? 'cloud.tool_called' : 'cloud.tool_not_called')}` : t('cloud.test_failed', { detail: errorText(result.detail) })}
            </p>
          {/if}
        </div>
      {/if}
    </article>
  {/each}
</div>

<Dialog.Root bind:open onOpenChange={(value) => { if (!value) key = ''; }}>
  <Dialog.Content class="sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title>{t(editing ? 'cloud.edit' : 'cloud.add')}</Dialog.Title>
      <Dialog.Description>{t(editing ? 'cloud.edit_description' : 'cloud.description')}</Dialog.Description>
    </Dialog.Header>
    <form class="flex flex-col gap-4" onsubmit={(event) => { event.preventDefault(); void save(); }}>
      <label class="flex flex-col gap-1 text-sm">{t('cloud.kind')}
        <select class="h-9 rounded-md border border-input bg-background px-2 text-sm" value={kind} disabled={busy || !!editing} onchange={(e) => pickKind((e.currentTarget as HTMLSelectElement).value as Kind)}>
          {#each KINDS as k (k.kind)}<option value={k.kind}>{kindLabel(k.kind)}</option>{/each}
        </select>
      </label>
      <label class="flex flex-col gap-1 text-sm">{t('cloud.name')}<Input bind:value={name} required maxlength={64} disabled={busy} /></label>
      <p class="text-xs text-muted-foreground">{t('cloud.name_hint')}</p>
      <label class="flex flex-col gap-1 text-sm">{t('cloud.address')}<Input type="url" bind:value={address} required={kind === 'custom'} disabled={busy} placeholder={kindInfo.url} /></label>
      {#if kind !== 'custom'}<p class="text-xs text-muted-foreground">{t('cloud.address_hint')}</p>{/if}
      <label class="flex flex-col gap-1 text-sm">{t('cloud.key')}<Input type="password" bind:value={key} autocomplete="new-password" required={needsKey} disabled={busy || removeKey} /></label>
      {#if editing?.key_set}<p class="text-xs text-muted-foreground">{t('cloud.key_hint')}</p>{/if}
      {#if kind === 'custom'}<p class="text-xs text-muted-foreground">{t('cloud.key_optional')}</p>{/if}
      {#if editing?.key_set}<label class="flex items-center gap-2 text-sm"><input type="checkbox" bind:checked={removeKey} disabled={busy} />{t('cloud.remove_key')}</label>{/if}
      {#if editing}<label class="flex items-center gap-2 text-sm"><input type="checkbox" bind:checked={enabled} disabled={busy} />{t('cloud.enabled')}</label>{/if}
      {#if formError}<p class="text-sm text-destructive" role="alert">{formError}</p>{/if}
      <Dialog.Footer>
        <Button type="button" variant="outline" disabled={busy} onclick={() => { open = false; key = ''; }}>{t('cloud.cancel')}</Button>
        <Button type="submit" disabled={busy || (needsKey && !key)}>{t('cloud.save')}</Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root open={!!removing} onOpenChange={(value) => { if (!value && !busy) removing = null; }}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t('cloud.remove')}</Dialog.Title>
      <Dialog.Description>{t('cloud.remove_confirm', { name: removing?.name ?? '' })}</Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" disabled={busy} onclick={() => { removing = null; }}>{t('cloud.cancel')}</Button>
      <Button variant="destructive" disabled={busy} onclick={remove}>{t('cloud.remove')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
