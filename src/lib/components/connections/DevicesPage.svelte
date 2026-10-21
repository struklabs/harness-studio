<script lang="ts">
  // Connections > Devices: pair phones and other computers (one-time code, approval prompt with
  // a verification code), see each device's role, access, models, engines, disk, workload and
  // reachability, unpair one device or all of them (lost phone), read the audit log, check where
  // a big model could run, and connect this computer to another Harness host. Protocol and
  // limits: harness_core::pairing; the same functions back the Control API (`/devices`,
  // `/pairing`).
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { errorText } from '#lib/errors.js';
  import { formatBytes } from '#lib/format.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { TriangleAlert } from '@lucide/svelte';
  import CopyButton from './CopyButton.svelte';

  type Role = 'inference_host' | 'client' | 'storage';
  type Preset = 'chat_only' | 'gateway_models' | 'admin';
  type Reach = 'online' | 'offline' | 'never';

  interface Report {
    models: string[];
    engines: string[];
    components: string[];
    disk_total_bytes: number | null;
    disk_free_bytes: number | null;
    ram_total_bytes: number | null;
    vram_total_bytes: number | null;
    vram_free_bytes: number | null;
    running_servers: number;
    queued_requests: number;
    address: string | null;
  }
  interface Device {
    id: string;
    name: string;
    platform: string;
    role: Role;
    preset: Preset;
    created_at: number;
    last_seen: number | null;
    reachability: Reach;
    models: string[];
    report: Report | null;
    missing_components: string[];
  }
  interface Pending {
    id: string;
    name: string;
    platform: string;
    role: Role;
    preset: Preset;
    models: string[];
    verification: string;
    expires_at: number;
  }
  interface Offer { offer: { id: string; code: string; expires_at: number }; pair_urls: string[]; reachable_from_lan: boolean }
  interface Audit { at: number; event: string; device_name: string | null; detail: string | null }
  interface Suggestion { fits_locally: boolean; local_bytes: number; hosts: { device_id: string; name: string; available_bytes: number; missing_components: string[]; reachability: Reach }[] }
  interface ClientClaim { base_url: string; claim_id: string; poll_secret: string; verification: string; allow_insecure_http: boolean }
  interface Progress { status: 'pending' | 'approved' | 'denied' | 'expired'; connected: { link_name: string; opencode_config: unknown } | null }

  const ROLES: Role[] = ['inference_host', 'client', 'storage'];
  const PRESETS: Preset[] = ['chat_only', 'gateway_models', 'admin'];
  const selectClass = 'w-full rounded-md border border-input bg-background px-3 py-2 text-sm';

  let devices = $state<Device[]>([]);
  let pending = $state<Pending[]>([]);
  let audit = $state<Audit[]>([]);
  let error = $state('');
  let busy = $state(false);
  let alive = true;

  let role = $state<Role>('client');
  let preset = $state<Preset>('chat_only');
  let models = $state('');
  let ackAdmin = $state(false);
  let offer = $state<Offer | null>(null);
  let offerError = $state('');

  let removing = $state<Device | null>(null);
  let revokingAll = $state(false);
  let showAudit = $state(false);

  let needGb = $state('');
  let suggestion = $state<Suggestion | null>(null);
  let suggestError = $state('');

  let cUrl = $state('');
  let cCode = $state('');
  let cName = $state('');
  let cLink = $state('');
  let cHttp = $state(false);
  let claim = $state<ClientClaim | null>(null);
  let cStatus = $state<Progress['status'] | ''>('');
  let cDone = $state<Progress['connected']>(null);
  let cError = $state('');
  let cTimer: ReturnType<typeof setTimeout> | null = null;

  const needsAck = $derived(preset === 'admin');
  const cHttpUrl = $derived(cUrl.trim().toLowerCase().startsWith('http:'));
  const gatewayOff = $derived(!app.gateway?.status.running);

  async function load() {
    try {
      const [d, p] = await Promise.all([
        invoke<Device[]>('devices_list'),
        invoke<Pending[]>('pairing_pending'),
      ]);
      if (!alive) return;
      devices = d;
      pending = p;
      error = '';
      if (showAudit) audit = await invoke<Audit[]>('devices_audit', { limit: 100 });
    } catch (e) { if (alive) error = errorText(e); }
  }

  onMount(() => {
    void load();
    void app.refreshGateway(true);
    cName = t('devices.default_name');
    const timer = setInterval(() => { if (!busy) void load(); }, 4000);
    return () => { alive = false; clearInterval(timer); if (cTimer) clearTimeout(cTimer); };
  });

  async function createOffer() {
    busy = true; offerError = ''; offer = null;
    try {
      offer = await invoke<Offer>('pairing_create_offer', {
        spec: {
          role,
          preset,
          models: models.split(',').map((m) => m.trim()).filter(Boolean),
          acknowledge_admin: needsAck && ackAdmin,
        },
      });
    } catch (e) { offerError = errorText(e); }
    finally { busy = false; }
  }

  async function act(fn: () => Promise<unknown>) {
    busy = true; error = '';
    try { await fn(); await load(); }
    catch (e) { error = errorText(e); }
    finally { busy = false; }
  }

  const approve = (p: Pending) => act(() => invoke('pairing_approve', { id: p.id }));
  const deny = (p: Pending) => act(() => invoke('pairing_deny', { id: p.id }));
  const changeRole = (d: Device, value: Role) => act(() => invoke('devices_update', { id: d.id, role: value }));

  async function unpair() {
    const d = removing;
    removing = null;
    if (d) await act(() => invoke('devices_revoke', { id: d.id }));
  }

  async function unpairAll() {
    revokingAll = false;
    await act(() => invoke('devices_revoke_all'));
    offer = null;
  }

  async function toggleAudit() {
    showAudit = !showAudit;
    if (showAudit) {
      try { audit = await invoke<Audit[]>('devices_audit', { limit: 100 }); }
      catch (e) { error = errorText(e); }
    }
  }

  async function suggest() {
    suggestError = ''; suggestion = null;
    const gb = Number(needGb.replace(',', '.'));
    if (!Number.isFinite(gb) || gb <= 0) { suggestError = t('devices.suggest_invalid'); return; }
    busy = true;
    try { suggestion = await invoke<Suggestion>('devices_suggest', { requiredBytes: Math.round(gb * 1024 ** 3) }); }
    catch (e) { suggestError = errorText(e); }
    finally { busy = false; }
  }

  async function connect() {
    busy = true; cError = ''; cDone = null; cStatus = '';
    try {
      claim = await invoke<ClientClaim>('pairing_connect_start', {
        url: cUrl, code: cCode, deviceName: cName, allowInsecureHttp: cHttpUrl && cHttp,
      });
      cCode = '';
      cStatus = 'pending';
      pollConnect();
    } catch (e) { cError = errorText(e); claim = null; }
    finally { busy = false; }
  }

  function pollConnect() {
    if (cTimer) clearTimeout(cTimer);
    cTimer = setTimeout(async () => {
      if (!claim || !alive) return;
      try {
        const progress = await invoke<Progress>('pairing_connect_poll', { linkName: cLink, claim });
        cStatus = progress.status;
        if (progress.status === 'pending') return pollConnect();
        if (progress.connected) cDone = progress.connected;
        claim = null;
      } catch (e) { cError = errorText(e); claim = null; }
    }, 2000);
  }

  function cancelConnect() {
    if (cTimer) clearTimeout(cTimer);
    claim = null; cStatus = '';
  }

  const component = (c: string) => t(`devices.component_${c}` as TKey);
  const when = (secs: number | null) => (secs ? new Date(secs * 1000).toLocaleString() : t('devices.never'));
  const split = (code: string) => `${code.slice(0, 3)} ${code.slice(3)}`;
  const configText = (c: unknown) => JSON.stringify(c, null, 2);
</script>

<div class="flex flex-col gap-6">
  <p class="text-sm text-muted-foreground">{t('devices.description')}</p>
  {#if error}<p class="text-sm text-destructive" role="alert">{error}</p>{/if}

  {#if pending.length > 0}
    <section class="flex flex-col gap-3" aria-labelledby="dev-pending">
      <h2 id="dev-pending" class="text-sm font-semibold">{t('devices.pending_title')}</h2>
      {#each pending as p (p.id)}
        <article class="flex flex-col gap-3 rounded-lg border border-warning/60 bg-card p-4">
          <p class="text-sm">{t('devices.pending_text', { name: p.name, platform: p.platform || '-' })}</p>
          <div class="flex flex-wrap items-center gap-4">
            <div>
              <p class="text-xs text-muted-foreground">{t('devices.verification')}</p>
              <p class="font-mono text-3xl tracking-widest" aria-label={t('devices.verification')}>{split(p.verification)}</p>
            </div>
            <div class="flex flex-col gap-1 text-xs text-muted-foreground">
              <span>{t('devices.role')}: {t(`devices.role_${p.role}`)}</span>
              <span>{t('devices.access')}: {t(`devices.preset_${p.preset}`)}</span>
              {#if p.models.length}<span>{t('devices.models')}: {p.models.join(', ')}</span>{/if}
            </div>
          </div>
          {#if p.preset === 'admin'}<p class="flex items-start gap-2 text-xs text-warning" role="status"><TriangleAlert class="size-4 shrink-0" />{t('devices.admin_warning')}</p>{/if}
          <p class="text-xs text-muted-foreground">{t('devices.pending_hint')}</p>
          <div class="flex flex-wrap gap-2">
            <Button size="sm" disabled={busy} onclick={() => approve(p)}>{t('devices.approve')}</Button>
            <Button size="sm" variant="outline" disabled={busy} onclick={() => deny(p)}>{t('devices.deny')}</Button>
          </div>
        </article>
      {/each}
    </section>
  {/if}

  <section class="flex flex-col gap-3 rounded-lg border border-border bg-card p-4" aria-labelledby="dev-pair">
    <h2 id="dev-pair" class="text-sm font-semibold">{t('devices.pair_title')}</h2>
    <p class="text-xs text-muted-foreground">{t('devices.pair_description')}</p>
    {#if gatewayOff}<p class="flex items-start gap-2 text-xs text-warning" role="status"><TriangleAlert class="size-4 shrink-0" />{t('devices.gateway_off')}</p>{/if}
    <div class="grid gap-3 sm:grid-cols-2">
      <label class="flex flex-col gap-1 text-sm">{t('devices.role')}
        <select class={selectClass} bind:value={role} disabled={busy}>
          {#each ROLES as r}<option value={r}>{t(`devices.role_${r}`)}</option>{/each}
        </select>
      </label>
      <label class="flex flex-col gap-1 text-sm">{t('devices.access')}
        <select class={selectClass} bind:value={preset} disabled={busy}>
          {#each PRESETS as p}<option value={p}>{t(`devices.preset_${p}`)}</option>{/each}
        </select>
      </label>
    </div>
    <p class="text-xs text-muted-foreground">{t(`devices.preset_${preset}_hint`)}</p>
    {#if preset !== 'admin'}
      <label class="flex flex-col gap-1 text-sm">{t('devices.allowed_models')}
        <Input bind:value={models} placeholder="qwen3-*, coder" disabled={busy} />
      </label>
      <p class="text-xs text-muted-foreground">{t('devices.allowed_models_hint')}</p>
    {:else}
      <label class="flex items-start gap-2 text-sm"><input type="checkbox" class="mt-1" bind:checked={ackAdmin} disabled={busy} />{t('devices.admin_ack')}</label>
    {/if}
    <div><Button disabled={busy || (needsAck && !ackAdmin)} onclick={createOffer}>{t('devices.create_code')}</Button></div>
    {#if offerError}<p class="text-sm text-destructive" role="alert">{offerError}</p>{/if}
    {#if offer}
      <div class="flex flex-col gap-2 rounded-md border border-border p-3">
        <p class="text-xs text-muted-foreground">{t('devices.code_label')}</p>
        <p class="font-mono text-3xl tracking-widest" data-testid="pair-code">{offer.offer.code}</p>
        <p class="text-xs text-muted-foreground">{t('devices.code_expires', { time: when(offer.offer.expires_at) })}</p>
        {#if !offer.reachable_from_lan}<p class="flex items-start gap-2 text-xs text-warning" role="status"><TriangleAlert class="size-4 shrink-0" />{t('devices.lan_off')}</p>{/if}
        {#if offer.pair_urls.length}
          <p class="text-xs text-muted-foreground">{t('devices.open_link')}</p>
          {#each offer.pair_urls as url}
            <div class="flex items-center gap-2"><span class="break-all font-mono text-xs">{url}</span><CopyButton text={url} ariaLabel={t('devices.copy_link')} /></div>
          {/each}
        {/if}
        <p class="text-xs text-muted-foreground">{t('devices.https_note')}</p>
      </div>
    {/if}
  </section>

  <section class="flex flex-col gap-3" aria-labelledby="dev-list">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <h2 id="dev-list" class="text-sm font-semibold">{t('devices.list_title')}</h2>
      <Button size="sm" variant="outline" disabled={busy || devices.length === 0} onclick={() => { revokingAll = true; }}>{t('devices.revoke_all')}</Button>
    </div>
    {#if devices.length === 0}<p class="text-sm text-muted-foreground">{t('devices.empty')}</p>{/if}
    {#each devices as d (d.id)}
      <article class="flex flex-col gap-3 rounded-lg border border-border bg-card p-4">
        <div class="flex flex-wrap items-center gap-2">
          <h3 class="text-sm font-semibold">{d.name}</h3>
          <Badge variant="outline">{t(`devices.role_${d.role}`)}</Badge>
          <Badge variant="outline">{t(`devices.reach_${d.reachability}`)}</Badge>
          <Badge variant="outline">{t(`devices.preset_${d.preset}`)}</Badge>
          {#if d.platform}<span class="text-xs text-muted-foreground">{d.platform}</span>{/if}
        </div>
        <dl class="grid gap-x-6 gap-y-1 text-xs sm:grid-cols-2">
          <div class="flex gap-2"><dt class="text-muted-foreground">{t('devices.last_seen')}</dt><dd>{when(d.last_seen)}</dd></div>
          <div class="flex gap-2"><dt class="text-muted-foreground">{t('devices.permissions')}</dt><dd>{t(`devices.preset_${d.preset}`)}{d.models.length ? `: ${d.models.join(', ')}` : ''}</dd></div>
          {#if d.report}
            <div class="flex gap-2"><dt class="text-muted-foreground">{t('devices.engines')}</dt><dd>{d.report.engines.join(', ') || '-'}</dd></div>
            <div class="flex gap-2"><dt class="text-muted-foreground">{t('devices.disk')}</dt><dd>{d.report.disk_free_bytes != null ? `${formatBytes(d.report.disk_free_bytes, 0)} / ${formatBytes(d.report.disk_total_bytes, 0)}` : '-'}</dd></div>
            <div class="flex gap-2"><dt class="text-muted-foreground">{t('devices.memory')}</dt><dd>{d.report.vram_total_bytes != null ? `${t('devices.vram')} ${formatBytes(d.report.vram_free_bytes, 0)} / ${formatBytes(d.report.vram_total_bytes, 0)}` : d.report.ram_total_bytes != null ? `${t('devices.ram')} ${formatBytes(d.report.ram_total_bytes, 0)}` : '-'}</dd></div>
            <div class="flex gap-2"><dt class="text-muted-foreground">{t('devices.workload')}</dt><dd>{t('devices.workload_value', { servers: d.report.running_servers, queued: d.report.queued_requests })}</dd></div>
            <div class="flex gap-2 sm:col-span-2"><dt class="text-muted-foreground">{t('devices.models')}</dt><dd class="break-words">{d.report.models.join(', ') || '-'}</dd></div>
            {#if d.report.address}<div class="flex gap-2 sm:col-span-2"><dt class="text-muted-foreground">{t('devices.address')}</dt><dd class="break-all font-mono">{d.report.address}</dd></div>{/if}
          {:else}
            <p class="text-muted-foreground sm:col-span-2">{t('devices.no_report')}</p>
          {/if}
        </dl>
        {#if d.missing_components.length}
          <p class="text-xs text-muted-foreground">{t('devices.missing_components', { components: d.missing_components.map(component).join(', ') })}</p>
        {/if}
        <div class="flex flex-wrap items-center gap-2">
          <select class="{selectClass} !w-auto" aria-label={t('devices.role')} value={d.role} disabled={busy} onchange={(e) => changeRole(d, e.currentTarget.value as Role)}>
            {#each ROLES as r}<option value={r}>{t(`devices.role_${r}`)}</option>{/each}
          </select>
          <Button size="sm" variant="outline" disabled={busy} onclick={() => { removing = d; }}>{t('devices.unpair')}</Button>
        </div>
      </article>
    {/each}
  </section>

  <section class="flex flex-col gap-3 rounded-lg border border-border bg-card p-4" aria-labelledby="dev-suggest">
    <h2 id="dev-suggest" class="text-sm font-semibold">{t('devices.suggest_title')}</h2>
    <p class="text-xs text-muted-foreground">{t('devices.suggest_description')}</p>
    <div class="flex flex-wrap items-end gap-2">
      <label class="flex flex-col gap-1 text-sm">{t('devices.suggest_need')}<Input class="w-32" bind:value={needGb} inputmode="decimal" placeholder="24" disabled={busy} /></label>
      <Button variant="outline" disabled={busy || !needGb} onclick={suggest}>{t('devices.suggest_check')}</Button>
    </div>
    {#if suggestError}<p class="text-sm text-destructive" role="alert">{suggestError}</p>{/if}
    {#if suggestion}
      {#if suggestion.fits_locally}
        <p class="text-sm" role="status">{t('devices.suggest_fits', { memory: formatBytes(suggestion.local_bytes, 0) })}</p>
      {:else if suggestion.hosts.length === 0}
        <p class="text-sm" role="status">{t('devices.suggest_none')}</p>
      {:else}
        <p class="text-sm" role="status">{t('devices.suggest_too_big', { memory: formatBytes(suggestion.local_bytes, 0) })}</p>
        <ul class="flex flex-col gap-1 text-sm">
          {#each suggestion.hosts as h (h.device_id)}
            <li>
              <span class="font-medium">{h.name}</span>
              <span class="text-muted-foreground"> · {t('devices.suggest_free', { memory: formatBytes(h.available_bytes, 0) })} · {t(`devices.reach_${h.reachability}`)}</span>
              {#if h.missing_components.length}<span class="block text-xs text-muted-foreground">{t('devices.missing_components', { components: h.missing_components.map(component).join(', ') })}</span>{/if}
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
  </section>

  <section class="flex flex-col gap-3 rounded-lg border border-border bg-card p-4" aria-labelledby="dev-connect">
    <h2 id="dev-connect" class="text-sm font-semibold">{t('devices.connect_title')}</h2>
    <p class="text-xs text-muted-foreground">{t('devices.connect_description')}</p>
    {#if claim}
      <p class="text-xs text-muted-foreground">{t('devices.connect_compare')}</p>
      <p class="font-mono text-3xl tracking-widest">{split(claim.verification)}</p>
      <p class="text-xs text-muted-foreground">{t('devices.connect_waiting')}</p>
      <div><Button size="sm" variant="outline" onclick={cancelConnect}>{t('devices.cancel')}</Button></div>
    {:else}
      <form class="flex flex-col gap-3" onsubmit={(e) => { e.preventDefault(); void connect(); }}>
        <label class="flex flex-col gap-1 text-sm">{t('devices.connect_address')}<Input bind:value={cUrl} required placeholder="http://192.168.1.20:11435" disabled={busy} /></label>
        <label class="flex flex-col gap-1 text-sm">{t('devices.connect_code')}<Input bind:value={cCode} required inputmode="numeric" autocomplete="off" placeholder="1234-5678" disabled={busy} /></label>
        <div class="grid gap-3 sm:grid-cols-2">
          <label class="flex flex-col gap-1 text-sm">{t('devices.connect_device_name')}<Input bind:value={cName} required maxlength={48} disabled={busy} /></label>
          <label class="flex flex-col gap-1 text-sm">{t('devices.connect_link_name')}<Input bind:value={cLink} required maxlength={64} placeholder="office-pc" disabled={busy} /></label>
        </div>
        {#if cHttpUrl}<label class="flex items-center gap-2 text-sm"><input type="checkbox" bind:checked={cHttp} disabled={busy} />{t('link.allow_http')}</label>{/if}
        <div><Button type="submit" disabled={busy || !cUrl || !cCode || !cLink || (cHttpUrl && !cHttp)}>{t('devices.connect_button')}</Button></div>
      </form>
    {/if}
    {#if cError}<p class="text-sm text-destructive" role="alert">{cError}</p>{/if}
    {#if cStatus === 'denied'}<p class="text-sm text-destructive" role="alert">{t('devices.connect_denied')}</p>{/if}
    {#if cStatus === 'expired'}<p class="text-sm text-destructive" role="alert">{t('devices.connect_expired')}</p>{/if}
    {#if cDone}
      <div class="flex flex-col gap-2 rounded-md border border-border p-3">
        <p class="text-sm" role="status">{t('devices.connect_done', { name: cDone.link_name })}</p>
        <p class="text-xs text-muted-foreground">{t('devices.opencode_hint')}</p>
        <pre class="max-h-56 overflow-auto rounded bg-muted p-2 text-xs">{configText(cDone.opencode_config)}</pre>
        <div><CopyButton text={configText(cDone.opencode_config)} label={t('devices.copy_config')} copiedLabel={t('devices.copied')} variant="outline" size="sm" /></div>
      </div>
    {/if}
  </section>

  <section class="flex flex-col gap-3" aria-labelledby="dev-audit">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <h2 id="dev-audit" class="text-sm font-semibold">{t('devices.audit_title')}</h2>
      <Button size="sm" variant="outline" onclick={toggleAudit}>{t(showAudit ? 'devices.audit_hide' : 'devices.audit_show')}</Button>
    </div>
    {#if showAudit}
      {#if audit.length === 0}<p class="text-sm text-muted-foreground">{t('devices.audit_empty')}</p>{/if}
      <ul class="flex flex-col gap-1 text-xs">
        {#each audit as a}
          <li class="flex flex-wrap gap-x-2"><span class="text-muted-foreground">{when(a.at)}</span><span class="font-medium">{t(`devices.audit_${a.event}` as TKey)}</span>{#if a.device_name}<span>{a.device_name}</span>{/if}{#if a.detail}<span class="text-muted-foreground">{a.detail}</span>{/if}</li>
        {/each}
      </ul>
    {/if}
  </section>
</div>

<Dialog.Root open={!!removing} onOpenChange={(value) => { if (!value && !busy) removing = null; }}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t('devices.unpair')}</Dialog.Title>
      <Dialog.Description>{t('devices.unpair_confirm', { name: removing?.name ?? '' })}</Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => { removing = null; }}>{t('devices.cancel')}</Button>
      <Button variant="destructive" disabled={busy} onclick={unpair}>{t('devices.unpair')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root open={revokingAll} onOpenChange={(value) => { if (!value && !busy) revokingAll = false; }}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t('devices.revoke_all')}</Dialog.Title>
      <Dialog.Description>{t('devices.revoke_all_confirm')}</Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => { revokingAll = false; }}>{t('devices.cancel')}</Button>
      <Button variant="destructive" disabled={busy} onclick={unpairAll}>{t('devices.revoke_all')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
