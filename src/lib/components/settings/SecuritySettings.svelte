<script lang="ts">
  import McpServers from '../connections/McpServers.svelte';
  import InstalledToolsSettings from './InstalledToolsSettings.svelte';
  import Agents from '../Agents.svelte';
  import PermissionsManager from './PermissionsManager.svelte';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  import { onMount, untrack } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { toast } from 'svelte-sonner';
  import { Copy, KeyRound, ShieldAlert, ShieldCheck, Wifi } from '@lucide/svelte';
  import type { AppSettings, NetworkOverview, ServerProfile } from '#lib/types.js';
  import { t, locale } from '#lib/i18n.svelte.js';
  import { bindScope, isValidBindHost, localizeError } from '#lib/network.js';
  import { requestSection } from '#lib/stores/sectionPage.svelte.js';
  import { app, DEFAULT_SERVER_ID } from '#lib/stores/app.svelte.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import * as RadioGroup from '#lib/components/ui/radio-group/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import { cn } from '#lib/utils.js';
  import type { SettingsDraft } from './form.svelte';
  import SettingGroup from './SettingGroup.svelte';
  import SettingRow from './SettingRow.svelte';

  let overviewMcpPage = $state<'servers' | 'import' | 'status'>('servers');

  interface Props {
    page?: string;
    settings: AppSettings;
    form: SettingsDraft;
    onSettingsUpdated: (updated: AppSettings) => void;
  }

  let { page = 'security-keys', settings, form, onSettingsUpdated }: Props = $props();

  let overview = $state<NetworkOverview | null>(null);

  async function loadOverview() {
    try {
      overview = await invoke<NetworkOverview>('get_network_overview');
    } catch {
      overview = null;
    }
  }

  onMount(() => {
    loadOverview();
    app.refreshProfiles();
  });

  // --- Default bind address -------------------------------------------------------------
  type BindMode = 'loopback' | 'lan' | 'custom';
  const LOOPBACK = '127.0.0.1';
  const LAN = '0.0.0.0';

  let host = $derived(form.get('host'));
  let customHost = $state(untrack(() => (['127.0.0.1', '0.0.0.0'].includes(form.get('host')) ? '' : form.get('host'))));
  let mode = $state<BindMode>(untrack(() => modeOf(form.get('host'))));
  let customValid = $derived(customHost.trim() === '' || isValidBindHost(customHost));
  let scope = $derived(bindScope(host));

  function modeOf(h: string): BindMode {
    return h === LOOPBACK ? 'loopback' : h === LAN ? 'lan' : 'custom';
  }

  // Follow external changes (save elsewhere, discard) when the user isn't editing a custom value.
  $effect(() => {
    const m = modeOf(host);
    untrack(() => {
      if (m !== 'custom' || mode !== 'custom') mode = m;
    });
  });

  function setMode(m: BindMode) {
    mode = m;
    if (m === 'loopback') form.set('host', LOOPBACK);
    else if (m === 'lan') form.set('host', LAN);
    else if (customHost.trim() && isValidBindHost(customHost)) form.set('host', customHost.trim());
  }

  function setCustom(v: string) {
    customHost = v;
    if (v.trim() && isValidBindHost(v)) form.set('host', v.trim());
  }

  const bindOptions: { v: BindMode; label: () => string; desc: () => string }[] = [
    { v: 'loopback', label: () => t('sec.bind_loopback'), desc: () => t('sec.bind_loopback_desc') },
    { v: 'lan', label: () => t('sec.bind_lan'), desc: () => t('sec.bind_lan_desc') },
    { v: 'custom', label: () => t('sec.bind_custom'), desc: () => t('sec.bind_custom_desc') },
  ];

  // --- CORS ----------------------------------------------------------------------------
  let corsText = $state(untrack(() => form.get('cors_origins').join('\n')));
  let corsList = $derived(
    corsText
      .split('\n')
      .map((s) => s.trim())
      .filter(Boolean),
  );

  $effect(() => {
    const saved = form.get('cors_origins');
    untrack(() => {
      if (saved.join('\n') !== corsList.join('\n')) corsText = saved.join('\n');
    });
  });

  function setCors(text: string) {
    corsText = text;
    form.set(
      'cors_origins',
      text
        .split('\n')
        .map((s) => s.trim())
        .filter(Boolean),
    );
  }

  // --- API keys ----------------------------------------------------------------------------
  interface KeyRow {
    id: string;
    name: string;
    host: string;
    keySet: boolean | null;
    running: boolean;
    profile: ServerProfile | null;
  }

  let keyRows = $derived.by<KeyRow[]>(() => {
    const active = (id: string) => app.servers.find((s) => s.id === id)?.active ?? null;
    const def = active(DEFAULT_SERVER_ID);
    const rows: KeyRow[] = [
      {
        id: DEFAULT_SERVER_ID,
        name: app.serverLabel(DEFAULT_SERVER_ID),
        host: def?.bind_host ?? (settings.server.host || LOOPBACK),
        // The Dashboard sends its key with each start; only a running server tells us.
        keySet: def ? def.api_key_set : null,
        running: !!def,
        profile: null,
      },
    ];
    for (const p of app.profiles) {
      const a = active(p.id);
      rows.push({
        id: p.id,
        name: p.name,
        host: a?.bind_host ?? (p.config.host || settings.server.host || LOOPBACK),
        keySet: a ? a.api_key_set || !!p.config.api_key_ref : !!p.config.api_key_ref,
        running: !!a,
        profile: p,
      });
    }
    return rows;
  });

  let keyBusy = $state<Record<string, boolean>>({});
  let confirmRegenerate = $state<string | null>(null);

  async function generateKey(row: KeyRow) {
    if (row.keySet && confirmRegenerate !== row.id) {
      confirmRegenerate = row.id;
      return;
    }
    confirmRegenerate = null;
    keyBusy[row.id] = true;
    try {
      app.setProfiles(await invoke<ServerProfile[]>('generate_profile_api_key', { profileId: row.id }));
      toast.success(t('sec.key_generated_toast'));
    } catch (e) {
      toast.error(t('sec.key_generate_failed', { error: localizeError(e) }));
    } finally {
      keyBusy[row.id] = false;
    }
  }

  async function copyKey(row: KeyRow) {
    try {
      const live = row.running ? await invoke<string | null>('get_server_api_key', { serverId: row.id }) : null;
      const value = live ?? (row.profile ? await invoke<string | null>('get_profile_api_key', { profileId: row.id }) : null);
      if (!value) return;
      await navigator.clipboard.writeText(value);
      toast.success(t('sec.key_copied_toast'));
    } catch (e) {
      toast.error(String(e));
    }
  }

  function openInDashboard(id: string) {
    requestSection('dashboard', 'network');
    app.selectServer(id);
    app.requestedTab = 'dashboard';
  }

  // --- "Open anyway" confirmations ------------------------------------------------------
  let acks = $derived(Object.entries(settings.network?.exposure_acks ?? {}));

  async function revoke(id: string) {
    try {
      onSettingsUpdated(await invoke<AppSettings>('revoke_lan_exposure', { serverId: id }));
      toast.success(t('sec.ack_revoked_toast'));
    } catch (e) {
      toast.error(localizeError(e));
    }
  }

  const fmtDate = (secs: number) => new Date(secs * 1000).toLocaleString(locale());
</script>

<div class="flex flex-col gap-6">
<div hidden={page !== 'network-listen'} class="flex flex-col gap-3">
  <SettingGroup title={t('sec.bind_title')} hint={t('sec.bind_desc')}>
    <div class="flex flex-col gap-3 p-3">
      <RadioGroup.Root
        class="grid grid-cols-3 gap-2 max-[1100px]:grid-cols-1"
        aria-label={t('sec.bind_title')}
        bind:value={() => mode, (v) => setMode(v as BindMode)}
      >
        {#each bindOptions as opt (opt.v)}
          <label
            for="bind-mode-{opt.v}"
            class={cn(
              'flex min-w-0 cursor-pointer items-start gap-2.5 rounded-md border p-3 transition-colors hover:bg-muted/50 has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring/50',
              mode === opt.v ? 'border-primary/40 bg-primary/5' : 'border-border',
            )}
          >
            <RadioGroup.Item id="bind-mode-{opt.v}" value={opt.v} class="mt-0.5" />
            <span class="flex min-w-0 flex-col gap-0.5">
              <span class="text-sm font-medium">{opt.label()}</span>
              <span class="text-xs text-muted-foreground">{opt.desc()}</span>
            </span>
          </label>
        {/each}
      </RadioGroup.Root>

      {#if mode === 'custom'}
        <div class="flex flex-col gap-1">
          <FieldHelp text={t('help.bind_host')}><Input
            id="bind-custom"
            class={cn('h-8 max-w-xs font-num text-xs', !customValid && 'border-destructive')}
            placeholder={t('sec.bind_custom_placeholder')}
            spellcheck={false}
            autocomplete="off"
            aria-invalid={!customValid}
            value={customHost}
            oninput={(e) => setCustom((e.currentTarget as HTMLInputElement).value)}
          /></FieldHelp>
          {#if !customValid}<p class="text-xs text-destructive" role="alert">{t('sec.bind_invalid')}</p>{/if}
        </div>
      {/if}

      {#if scope === 'lan'}
        <div class="flex flex-col gap-2 rounded-md border border-border bg-muted/30 p-3 text-xs">
          <p class="flex items-center gap-1.5 font-medium">
            <Wifi class="size-3.5 text-muted-foreground" />
            {t('sec.lan_addresses_title')}
          </p>
          {#if overview && overview.lan_addresses.length > 0}
            <ul class="flex flex-wrap gap-1.5">
              {#each overview.lan_addresses as a (a.ip)}
                <li>
                  <Badge variant="outline" class="font-num" title={a.interface}>{a.ip}</Badge>
                  <span class="ml-1 text-muted-foreground">{a.interface}</span>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="text-muted-foreground">{t('sec.lan_addresses_none')}</p>
          {/if}
          <p class="text-muted-foreground">{t('sec.lan_key_note')}</p>
          <p class="text-muted-foreground">{t('sec.firewall_note')}</p>
        </div>
      {/if}
    </div>
  </SettingGroup>

</div>
<div hidden={page !== 'security-keys'} class="flex flex-col gap-3">
  <SettingGroup title={t('sec.keys_title')} showTitle={false} note={t('sec.keys_desc')}>
    {#each keyRows as row (row.id)}
      {@const rowScope = bindScope(row.host)}
      <SettingRow label={row.name}>
        {#snippet extra()}
          <div class="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
            <span class="font-num">{row.host}</span>
            <Badge variant="outline" class={rowScope === 'lan' ? 'text-warning' : ''}>
              {rowScope === 'lan' ? t('sec.scope_lan') : t('sec.scope_loopback')}
            </Badge>
            {#if row.keySet === true}
              <Badge variant="outline" class="text-success">
                <ShieldCheck class="size-3" />
                {row.running ? t('sec.key_running_with') : t('sec.key_set')}
              </Badge>
            {:else if row.keySet === false}
              <Badge variant="outline" class={rowScope === 'lan' ? 'text-destructive' : ''}>
                {row.running ? t('sec.key_running_without') : t('sec.key_unset')}
              </Badge>
            {/if}
          </div>
          {#if confirmRegenerate === row.id}
            <p class="text-xs text-warning" role="alert">{t('sec.key_regenerate_confirm')}</p>
          {/if}
        {/snippet}
        {#if row.profile}
          <Button variant="outline" size="sm" disabled={keyBusy[row.id]} onclick={() => generateKey(row)}>
            <KeyRound />
            {row.keySet ? t('sec.key_regenerate') : t('sec.key_generate')}
          </Button>
          {#if confirmRegenerate === row.id}
            <Button variant="ghost" size="sm" onclick={() => (confirmRegenerate = null)}>{t('settings.cancel')}</Button>
          {/if}
        {/if}
        {#if row.keySet}
          <Button variant="ghost" size="icon-sm" aria-label={t('sec.key_copy')} title={t('sec.key_copy')} onclick={() => copyKey(row)}>
            <Copy />
          </Button>
        {/if}
        <Button variant="ghost" size="sm" onclick={() => openInDashboard(row.id)}>{t('sec.key_set_in_dashboard')}</Button>
      </SettingRow>
    {:else}
      <p class="px-4 py-3 text-xs text-muted-foreground">{t('sec.keys_none')}</p>
    {/each}
  </SettingGroup>

</div>
<div hidden={page !== 'network-access'} class="flex flex-col gap-3">
  <SettingGroup title={t('sec.cors_title')} note={t('sec.cors_desc')}>
    {#snippet actions()}
      {#if overview?.cors_supported === true}
        <Badge variant="outline" class="text-success">{t('sec.cors_supported')}</Badge>
      {:else if overview?.cors_supported === false}
        <Badge variant="outline" class="text-destructive">{t('sec.cors_unsupported')}</Badge>
      {:else}
        <Badge variant="outline">{t('sec.cors_unknown')}</Badge>
      {/if}
    {/snippet}
    <div class="flex flex-col gap-2 p-3">
      <Textarea
        id="cors-origins"
        rows={4}
        class="font-mono text-xs"
        spellcheck={false}
        placeholder={t('sec.cors_placeholder')}
        value={corsText}
        oninput={(e) => setCors((e.currentTarget as HTMLTextAreaElement).value)}
        aria-describedby="cors-help"
      />
      <div id="cors-help" class="flex flex-col gap-1 text-xs text-muted-foreground">
        {#if corsList.length === 0}
          <p>{t('sec.cors_default_loopback')}</p>
          <p>{t('sec.cors_default_lan')}</p>
        {/if}
        <p>{t('sec.cors_special')}</p>
        {#if corsList.length > 1}
          <p class="flex items-start gap-1.5 text-warning" role="status">
            <ShieldAlert class="mt-0.5 size-3.5 shrink-0" />
            {t('sec.cors_multi_warning')}
          </p>
          <p>{t('sec.cors_multi_gateway')}</p>
        {/if}
      </div>
    </div>
  </SettingGroup>

</div>
<div hidden={page !== 'security-permissions'} class="flex flex-col gap-3">
  {#if page === 'security-permissions'}
    <PermissionsManager />
    <h3 class="font-medium">{t('trust.overview')}</h3>
    <details><summary>{t('trust.mcp')}</summary><McpServers page={overviewMcpPage} onNavigate={(next) => { if (next === 'servers' || next === 'import' || next === 'status') overviewMcpPage = next; }} /></details>
    <details><summary>{t('trust.tools')}</summary><InstalledToolsSettings /></details>
    <details><summary>{t('trust.agents')}</summary><Agents /></details>
  {/if}
  <SettingGroup title={t('sec.acks_title')} note={t('sec.acks_desc')}>
    {#each acks as [id, ack] (id)}
      <SettingRow label={app.serverLabel(id)} hint={t('sec.ack_at', { date: fmtDate(ack.acknowledged_at), host: ack.host })}>
        <Badge variant="outline" class="text-warning"><ShieldAlert class="size-3" /> {t('sec.auth_none')}</Badge>
        <Button variant="outline" size="sm" onclick={() => revoke(id)}>{t('sec.ack_revoke')}</Button>
      </SettingRow>
    {:else}
      <p class="px-4 py-3 text-xs text-muted-foreground">{t('sec.acks_none')}</p>
    {/each}
  </SettingGroup>
</div>
</div>
