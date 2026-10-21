<script lang="ts">
  import { Button } from '#lib/components/ui/button/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { t, getLanguage, type TKey } from '#lib/i18n.svelte.js';
  import { connections } from '../connections.svelte.ts';
  import {
    HarnessError,
    analytics,
    can,
    devices,
    gatewayModels,
    loadModel,
    moduleAction,
    modules,
    permissions,
    serverState,
    servers,
    setTrust,
    unloadModel,
    type Device,
    type GatewayModel,
    type ModelStats,
    type ModuleView,
    type ServerView,
    type Subject,
  } from '../harness.ts';
  import { bytes, errorText } from '../text.ts';
  import HarnessPicker from './HarnessPicker.svelte';

  let selected = $state(connections.harness.find((c) => c.apiUrl)?.id ?? connections.harness[0]?.id ?? '');
  const connection = $derived(connections.harness.find((c) => c.id === selected) ?? null);
  const identity = $derived(connection ? connections.identities[connection.id] ?? null : null);

  let models = $state<GatewayModel[]>([]);
  let running = $state<ServerView[]>([]);
  let hw = $state<Device[]>([]);
  let mods = $state<ModuleView[]>([]);
  let subjects = $state<Subject[] | null>(null);
  let stats = $state<ModelStats[]>([]);
  let errors = $state<Record<string, string | null>>({});
  let pending = $state<string | null>(null);

  async function section<T>(name: string, run: () => Promise<T>, set: (v: T) => void) {
    try {
      set(await run());
      errors[name] = null;
    } catch (e) {
      errors[name] = e instanceof HarnessError ? e.key : 'mobile.transport';
    }
  }

  async function refresh() {
    const c = connection;
    if (!c) return;
    if (!(c.id in connections.identities)) await connections.refreshIdentity(c);
    await Promise.all([
      section('models', () => gatewayModels(c), (v) => (models = v)),
      c.apiUrl ? section('servers', () => servers(c), (v) => (running = v)) : null,
      c.apiUrl ? section('devices', () => devices(c), (v) => (hw = v.devices)) : null,
      c.apiUrl ? section('modules', () => modules(c), (v) => (mods = v)) : null,
      c.apiUrl ? section('perf', () => analytics(c), (v) => (stats = v.models ?? [])) : null,
      c.apiUrl && can(identity, 'admin') ? section('permissions', () => permissions(c), (v) => (subjects = v)) : null,
    ]);
  }

  $effect(() => {
    void connection;
    void identity;
    void refresh();
  });

  async function act(key: string, run: () => Promise<unknown>) {
    pending = key;
    errors.action = null;
    try {
      await run();
      await refresh();
    } catch (e) {
      errors.action = e instanceof HarnessError ? e.key : 'mobile.transport';
    } finally {
      pending = null;
    }
  }

  const servedBy = (alias: string) => running.filter((s) => s.served_alias === alias && serverState(s.status) === 'running');
  // Module manifests name their texts by translation key (`{tr: 'modules.speech', en: …}`).
  const keyText = (key: string | undefined, fallback: string) => {
    const text = key ? t(key as TKey) : fallback;
    return text === key ? fallback : text;
  };
  const moduleName = (m: ModuleView) =>
    typeof m.manifest.name === 'string' ? keyText(m.manifest.name, m.manifest.id) : keyText(m.manifest.name[getLanguage()] ?? m.manifest.name.en, m.manifest.id);
</script>

{#snippet failed(name: string)}
  {#if errors[name]}<p class="text-xs text-destructive">{errorText(errors[name])}</p>{/if}
{/snippet}

{#snippet gap(title: TKey)}
  <section class="space-y-1">
    <h2 class="text-sm font-semibold">{t(title)}</h2>
    <p class="text-xs text-muted-foreground">{t('mobile.manage.needs_api')}</p>
  </section>
{/snippet}

<div class="space-y-6 p-4">
  <HarnessPicker bind:selected />
  {#if !connection}
    <p class="text-sm text-muted-foreground">{t('mobile.needs_harness')}</p>
  {:else}
    {#if identity}
      <p class="text-xs text-muted-foreground">{t('mobile.manage.role', { scopes: identity.scopes.join(', ') || '–' })}</p>
    {/if}
    {#if !connection.apiUrl}<p class="text-xs text-muted-foreground">{t('mobile.err.no_api')}</p>{/if}
    {#if errors.action}<p class="text-xs text-destructive" role="alert">{errorText(errors.action)}</p>{/if}

    <section class="space-y-2">
      <h2 class="text-sm font-semibold">{t('mobile.manage.models')}</h2>
      {@render failed('models')}
      {#each models as m (m.id)}
        {@const on = servedBy(m.id)}
        <div class="flex items-center justify-between gap-2 rounded-lg border border-border bg-card p-2">
          <div class="min-w-0">
            <div class="truncate text-sm">{m.id}</div>
            {#if on.length}<div class="text-xs text-muted-foreground">{t('mobile.manage.running_on', { servers: on.map((s) => s.name).join(', ') })}</div>{/if}
          </div>
          {#if connection.apiUrl && can(identity, 'servers:control') && !m.id.includes(':')}
            {#if on.length}
              <Button size="sm" variant="outline" disabled={pending !== null} onclick={() => act(`unload:${m.id}`, () => unloadModel(connection, m.id))}>{t('mobile.manage.unload')}</Button>
            {:else}
              <Button size="sm" variant="outline" disabled={pending !== null} onclick={() => act(`load:${m.id}`, () => loadModel(connection, m.id))}>{pending === `load:${m.id}` ? t('mobile.loading') : t('mobile.manage.load')}</Button>
            {/if}
          {/if}
        </div>
      {/each}
    </section>

    {#if connection.apiUrl}
      <section class="space-y-2">
        <h2 class="text-sm font-semibold">{t('mobile.manage.devices')}</h2>
        {@render failed('devices')}
        {#each hw as d (d.index)}
          <div class="rounded-lg border border-border bg-card p-2 text-sm">
            {d.name}
            <span class="text-xs text-muted-foreground">{[d.backend, d.memory_total_bytes ? bytes(d.memory_total_bytes) : null].filter(Boolean).join(' · ')}</span>
          </div>
        {/each}
      </section>

      <section class="space-y-2">
        <h2 class="text-sm font-semibold">{t('mobile.manage.performance')}</h2>
        {@render failed('perf')}
        {#each stats as s (s.alias)}
          <div class="rounded-lg border border-border bg-card p-2 text-xs">
            <div class="text-sm">{s.alias}</div>
            <div class="text-muted-foreground">
              {t('mobile.manage.perf_line', { requests: s.requests, errors: s.errors, tps: s.avg_gen_tps?.toFixed(1) ?? '–', ttft: s.ttft_p50_ms?.toFixed(0) ?? '–' })}
            </div>
          </div>
        {:else}
          {#if !errors.perf}<p class="text-xs text-muted-foreground">{t('mobile.manage.no_requests')}</p>{/if}
        {/each}
      </section>

      <section class="space-y-2">
        <h2 class="text-sm font-semibold">{t('mobile.manage.modules')}</h2>
        {@render failed('modules')}
        {#each mods.filter((m) => !m.manifest.planned) as m (m.manifest.id)}
          <div class="flex items-center justify-between gap-2 rounded-lg border border-border bg-card p-2">
            <div class="min-w-0">
              <div class="truncate text-sm">{moduleName(m)}</div>
              <div class="text-xs text-muted-foreground">{keyText(`modules.${m.state.status}`, m.state.status)}{m.state.installed_version ? ` · ${m.state.installed_version}` : ''}</div>
            </div>
            {#if can(identity, 'admin')}
              {#if m.update_available}
                <Button size="sm" variant="outline" disabled={pending !== null} onclick={() => act(`mod:${m.manifest.id}`, () => moduleAction(connection, m.manifest.id, 'update'))}>{t('mobile.manage.update')}</Button>
              {:else if !m.state.installed_version}
                <Button size="sm" variant="outline" disabled={pending !== null} onclick={() => act(`mod:${m.manifest.id}`, () => moduleAction(connection, m.manifest.id, 'install'))}>{t('mobile.manage.install')}</Button>
              {/if}
            {/if}
          </div>
        {/each}
      </section>

      <section class="space-y-2">
        <h2 class="text-sm font-semibold">{t('mobile.manage.approvals')}</h2>
        {#if !can(identity, 'admin')}
          <p class="text-xs text-muted-foreground">{t('mobile.manage.needs_admin')}</p>
        {:else}
          {@render failed('permissions')}
          {#each subjects ?? [] as s (`${s.kind}:${s.id}`)}
            <div class="flex items-center justify-between gap-2 rounded-lg border border-border bg-card p-2">
              <div class="min-w-0">
                <div class="truncate text-sm">{s.label}</div>
                <div class="text-xs text-muted-foreground">{s.kind} · {s.trust ?? s.inherent}</div>
              </div>
              {#if s.trust === null}
                <Badge variant="outline">{t('mobile.manage.default_trust')}</Badge>
              {:else}
                <Button size="sm" variant="ghost" disabled={pending !== null} onclick={() => act(`trust:${s.id}`, () => setTrust(connection, s, null))}>{t('mobile.manage.reset_trust')}</Button>
              {/if}
            </div>
          {/each}
          <p class="text-xs text-muted-foreground">{t('mobile.manage.tool_approvals_gap')}</p>
        {/if}
      </section>
    {/if}

    {@render gap('mobile.manage.agents')}
    {@render gap('mobile.manage.training')}
    {@render gap('mobile.manage.settings')}
  {/if}
</div>
