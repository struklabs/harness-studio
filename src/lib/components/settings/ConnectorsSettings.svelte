<script lang="ts">
  // Settings > Tools > Connectors: sign in to Gmail, Google Drive, Google Calendar and Excel
  // (Microsoft Graph) with the user's own OAuth client. Consent comes first, tokens go to the
  // OS keychain only, and write tools stay off unless the user turns them on here.
  import { onMount } from 'svelte';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { invoke } from '#lib/ipc.js';
  import { t } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import OfficialSuggestions from '#lib/components/tools/OfficialSuggestions.svelte';
  import SettingGroup from './SettingGroup.svelte';

  interface Connector {
    id: string; name: string; family: 'google' | 'microsoft'; connected: boolean; write: boolean;
    scopes: string[]; read_scopes: string[]; write_scopes: string[]; client_id_set: boolean;
    client_source: 'settings' | 'env' | null; redirect_uri: string; revocable_here: boolean; tools: string[];
  }
  const DOCS = 'https://github.com/struklabs/harness-studio/blob/main/docs/guide/en/connectors.md';
  const WRITE_TOOLS = new Set(['gmail_send', 'drive_create_text_file', 'calendar_create_event', 'excel_write_range']);

  let { showTitle = true }: { showTitle?: boolean } = $props();
  let items = $state<Connector[]>([]);
  let busy = $state(false);
  let waiting = $state<string | null>(null);
  let message = $state('');
  let error = $state('');
  let opened = $state<string | null>(null);
  let allowWrite = $state(false);
  let understood = $state(false);
  let googleId = $state('');
  let googleSecret = $state('');
  let microsoftId = $state('');
  let more = $state('');

  const load = async () => { items = await invoke<Connector[]>('connectors_status'); };
  onMount(() => { void load().catch(e => error = localizeError(e)); });
  async function act(work: () => Promise<void>) {
    busy = true; error = ''; message = '';
    try { await work(); } catch (e) { error = localizeError(e); } finally { busy = false; }
  }
  const readTools = (c: Connector) => c.tools.filter(n => !WRITE_TOOLS.has(n)).join(', ');
  const writeTools = (c: Connector) => c.tools.filter(n => WRITE_TOOLS.has(n)).join(', ');
  const sourceLabel = (family: 'google' | 'microsoft'): string => {
    const c = items.find(i => i.family === family);
    return t(c?.client_source === 'settings' ? 'connector.source_settings' : c?.client_source === 'env' ? 'connector.source_env' : 'connector.source_none');
  };
  const saveClient = () => act(async () => {
    if (googleId.trim() || googleSecret.trim()) await invoke('connectors_set_client', { family: 'google', clientId: googleId.trim(), clientSecret: googleSecret.trim() || null });
    if (microsoftId.trim()) await invoke('connectors_set_client', { family: 'microsoft', clientId: microsoftId.trim(), clientSecret: null });
    googleId = ''; googleSecret = ''; microsoftId = '';
    await load(); message = t('connector.client_saved');
  });
  const open = (id: string) => { opened = opened === id ? null : id; allowWrite = false; understood = false; };
  const connect = (c: Connector) => act(async () => {
    waiting = c.id;
    try { await invoke('connectors_connect', { id: c.id, write: allowWrite, consent: understood }); opened = null; message = t('connector.connected_ok', { name: c.name }); }
    finally { waiting = null; await load(); }
  });
  const cancel = (id: string) => void invoke('connectors_cancel', { id });
  const disconnect = (c: Connector) => act(async () => {
    const done = await invoke<{ remote_revoked: boolean }>('connectors_disconnect', { id: c.id });
    await load();
    message = t(done.remote_revoked ? 'connector.revoked_remote' : 'connector.revoked_local', { name: c.name });
  });
</script>

<div class="flex flex-col gap-4">
  <SettingGroup title={t('connector.title')} {showTitle} hint={t('connector.intro')}>
    <div class="flex flex-col gap-3 p-4">
      <p class="text-sm">{t('connector.privacy')}</p>
      <p class="text-xs text-muted-foreground">{t('connector.policy_note')}</p>
      <div><Button variant="outline" size="sm" onclick={() => void openUrl(DOCS)}>{t('connector.docs')}</Button></div>
      {#if message}<p role="status" class="text-sm break-words">{message}</p>{/if}
      {#if error}<p role="alert" class="text-sm text-destructive break-words">{error}</p>{/if}
    </div>
  </SettingGroup>

  <SettingGroup title={t('connector.client_title')} hint={t('connector.client_hint')}>
    <form class="flex flex-col gap-3 p-4" onsubmit={(e) => { e.preventDefault(); void saveClient(); }}>
      <label class="flex flex-col gap-1 text-xs text-muted-foreground">{t('connector.client_google')} · {sourceLabel('google')}
        <Input bind:value={googleId} autocomplete="off" spellcheck="false" placeholder="1234567890-abc.apps.googleusercontent.com" />
      </label>
      <label class="flex flex-col gap-1 text-xs text-muted-foreground">{t('connector.client_google_secret')}
        <Input type="password" bind:value={googleSecret} autocomplete="new-password" />
      </label>
      <label class="flex flex-col gap-1 text-xs text-muted-foreground">{t('connector.client_microsoft')} · {sourceLabel('microsoft')}
        <Input bind:value={microsoftId} autocomplete="off" spellcheck="false" placeholder="00000000-0000-0000-0000-000000000000" />
      </label>
      <p class="text-xs text-muted-foreground">{t('connector.redirect', { google: 'http://127.0.0.1/callback', microsoft: 'http://localhost/callback' })}</p>
      <Button type="submit" class="self-end" size="sm" disabled={busy}>{t('connector.client_save')}</Button>
    </form>
  </SettingGroup>

  {#each items as c (c.id)}
    <section class="flex flex-col gap-3 rounded-lg border p-4" aria-label={c.name}>
      <div class="flex flex-wrap items-center justify-between gap-2">
        <h3 class="font-semibold">{c.name}</h3>
        <Badge variant={c.connected ? 'default' : 'outline'}>{t(c.connected ? 'connector.status_connected' : 'connector.status_not')}</Badge>
      </div>
      <p class="text-xs text-muted-foreground">{t('connector.tools_label', { tools: c.connected ? c.tools.filter(n => c.write || !WRITE_TOOLS.has(n)).join(', ') : readTools(c) })}</p>
      {#if c.connected}
        <p class="text-xs">{t(c.write ? 'connector.write_on' : 'connector.write_off')}</p>
        <p class="text-xs break-all text-muted-foreground">{t('connector.scopes_granted', { scopes: c.scopes.join(' ') })}</p>
        <div><Button variant="outline" size="sm" disabled={busy} onclick={() => void disconnect(c)}>{t('connector.disconnect')}</Button></div>
      {:else if waiting === c.id}
        <p role="status" class="text-sm">{t('connector.waiting')}</p>
        <div><Button variant="outline" size="sm" onclick={() => cancel(c.id)}>{t('connector.cancel')}</Button></div>
      {:else}
        {#if !c.client_id_set}<p class="text-xs text-warning">{t('connector.needs_client')}</p>{/if}
        <div><Button variant="outline" size="sm" disabled={busy || !c.client_id_set} onclick={() => open(c.id)}>{t('connector.connect')}</Button></div>
        {#if opened === c.id}
          <div class="flex flex-col gap-3 rounded border p-3" role="group" aria-label={t('connector.consent_title', { name: c.name })}>
            <h4 class="text-sm font-semibold">{t('connector.consent_title', { name: c.name })}</h4>
            <p class="text-sm">{t('connector.consent_read', { tools: readTools(c) })}</p>
            <label class="flex gap-2 text-sm"><input type="checkbox" bind:checked={allowWrite} disabled={busy} />{t('connector.consent_write', { tools: writeTools(c) })}</label>
            <p class="text-xs text-muted-foreground">{t('connector.privacy')}</p>
            <label class="flex gap-2 text-sm"><input type="checkbox" bind:checked={understood} disabled={busy} />{t('connector.consent_confirm')}</label>
            <div class="flex gap-2">
              <Button size="sm" disabled={busy || !understood} onclick={() => void connect(c)}>{t('connector.consent_continue')}</Button>
              <Button size="sm" variant="outline" onclick={() => opened = null}>{t('connector.cancel')}</Button>
            </div>
          </div>
        {/if}
      {/if}
    </section>
  {/each}

  <SettingGroup title={t('connector.more_title')} hint={t('connector.more_hint')}>
    <div class="flex flex-col gap-3 p-4">
      <Input bind:value={more} aria-label={t('connector.more_title')} placeholder={t('connector.more_ph')} />
      <OfficialSuggestions kind="connector" query={more} />
    </div>
  </SettingGroup>
</div>
