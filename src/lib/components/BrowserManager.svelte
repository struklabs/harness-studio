<script lang="ts">
  import { onMount } from 'svelte';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { t, setLanguage, type TKey } from '#lib/i18n.svelte.js';
  import ServerOperations from './ServerOperations.svelte';
  import { hideSplash } from '#lib/splash.js';
  import { browserSession, browserLogin, browserLogout, control, controlEvents, ControlError } from '#lib/transport.js';

  type Page = 'dashboard' | 'library' | 'downloads' | 'settings' | 'tokens' | 'operations';
  type Status = string | { error?: string; Error?: string };
  interface Server { id: string; name: string; status: Status; endpoint?: string; failure?: { message?: string; code?: string } }
  interface Model { model: { id: string; name: string; size_bytes: number; path: string; source?: string } }
  interface Definition { id: string; name: string; alias: string; base?: { kind: string; path?: string } }
  interface Download { id: number; repo_id: string; variant_name: string; downloaded_bytes: number; total_bytes: number; status: Status }
  interface Token { id: string; name: string; scopes: string[] }
  const pages: Page[] = ['dashboard', 'library', 'downloads', 'settings', 'tokens', 'operations'];
  const availableScopes = ['read', 'inference', 'models:write', 'downloads:write', 'servers:write', 'servers:control', 'conversations', 'tools', 'admin'];
  let page = $state<Page>('dashboard');
  let authenticated = $state(false);
  let setup = $state(false);
  let initialized = $state(false);
  let connected = $state(false);
  let busy = $state(false);
  let error = $state('');
  let notice = $state('');
  let address = $state('');
  let password = $state('');
  let code = $state('');
  let servers = $state<Server[]>([]);
  let models = $state<Model[]>([]);
  let definitions = $state<Definition[]>([]);
  let downloads = $state<Download[]>([]);
  let tokens = $state<Token[]>([]);
  let repo = $state('');
  let file = $state('');
  let context = $state(0);
  let layers = $state(999);
  let tokenName = $state('');
  let scopes = $state<string[]>(['read']);
  let newToken = $state('');
  let selectedModel = $state<Model | null>(null);
  let alias = $state('');
  let stopEvents: (() => void) | undefined;
  let disposed = false;
  let refreshing = false;
  let refreshAgain = false;

  function report(e: unknown) {
    if (e instanceof ControlError) error = `${t(e.code as TKey, { ...e.params, seconds: e.retryAfter || e.params.seconds || 0 })} (${e.status})`;
    else error = e instanceof Error ? e.message : String(e);
  }
  function status(value: Status): string { return typeof value === 'string' ? value : value.error ?? value.Error ?? JSON.stringify(value); }
  function stopped(value: Status): boolean { return ['stopped', 'error'].includes(status(value).toLowerCase()) || typeof value !== 'string'; }
  function bytes(n: number): string { return `${(n / 1024 ** 3).toFixed(2)} GiB`; }

  async function refresh() {
    if (!authenticated || disposed) return;
    if (refreshing) { refreshAgain = true; return; }
    refreshing = true;
    const selected = page;
    try {
      if (selected === 'dashboard') servers = await control<Server[]>('/servers');
      if (selected === 'library') {
        const data = await control<{ entries: Model[]; definitions: Definition[]; state_error?: string }>('/models');
        models = data.entries; definitions = data.definitions;
        if (data.state_error) error = data.state_error;
      }
      if (selected === 'downloads') downloads = await control<Download[]>('/downloads');
      if (selected === 'settings') {
        const data = await control<{ default_ctx: number; default_ngl: number }>('/browser/settings');
        context = data.default_ctx; layers = data.default_ngl;
      }
      if (selected === 'tokens') tokens = await control<Token[]>('/tokens');
      connected = true;
    } catch (e) { report(e); connected = false; }
    finally {
      refreshing = false;
      if (refreshAgain) { refreshAgain = false; void refresh(); }
    }
  }
  async function select(next: Page) { page = next; newToken = ''; error = ''; notice = ''; await refresh(); }
  function startEvents() {
    stopEvents?.();
    stopEvents = controlEvents(() => { connected = true; if (page !== 'settings' && page !== 'tokens') void refresh(); }, () => {
      connected = false;
      void browserSession().then(s => { if (!s.authenticated) expire(); }).catch(report);
    });
  }
  function expire() { authenticated = false; password = ''; code = ''; newToken = ''; servers = []; models = []; definitions = []; tokens = []; downloads = []; selectedModel = null; stopEvents?.(); }
  async function action(fn: () => Promise<unknown>, reload = true) {
    if (busy) return;
    busy = true; error = ''; notice = '';
    try { await fn(); if (reload) await refresh(); }
    catch (e) { report(e); }
    finally { busy = false; }
  }
  async function signIn(event: SubmitEvent) {
    event.preventDefault();
    await action(async () => {
      await browserLogin(password, setup ? code : undefined);
      password = ''; code = '';
      // Verify the browser accepted the Secure cookie before showing any management controls.
      const session = await browserSession();
      if (!session.authenticated) throw new Error(t('browser.secure'));
      authenticated = true; setup = false;
      startEvents();
    });
  }
  async function signOut(all = false) {
    await action(async () => { await browserLogout(all); expire(); }, false);
  }
  async function loadModel(entry: Model) {
    const existing = definitions.find(d => d.base?.kind === 'local' && d.base.path === entry.model.path);
    if (existing) await action(() => control('/load', 'POST', { definition_id: existing.id }));
    else {
      selectedModel = entry;
      alias = entry.model.name.replace(/\.[^.]+$/, '').replace(/[^A-Za-z0-9._:-]/g, '-');
    }
  }
  async function createAndLoad(event: SubmitEvent) {
    event.preventDefault();
    if (!selectedModel) return;
    const model = selectedModel;
    await action(async () => {
      const definition = await control<Definition>('/definitions', 'POST', { alias, base: { kind: 'local', path: model.model.path } });
      selectedModel = null;
      await control('/load', 'POST', { definition_id: definition.id });
    });
  }
  async function deleteModel(id: string) {
    if (window.confirm(t('browser.delete_confirm'))) await action(() => control(`/models/${encodeURIComponent(id)}`, 'DELETE'));
  }
  onMount(() => {
    address = location.origin;
    setLanguage(localStorage.getItem('harness_lang') === 'tr' ? 'tr' : 'en');
    hideSplash();
    window.addEventListener('harness-session-expired', expire);
    void browserSession().then(session => {
      if (disposed) return;
      setup = session.setup_required; authenticated = session.authenticated;
      if (authenticated) { startEvents(); void refresh(); }
    }).catch(report).finally(() => { initialized = true; });
    return () => { disposed = true; stopEvents?.(); window.removeEventListener('harness-session-expired', expire); };
  });
</script>

<svelte:head><title>{t('browser.title')}</title></svelte:head>
<div class="browser-shell">
  <header>
    <div><h1>{t('browser.title')}</h1><p>{t('browser.connection')}: <strong>{address}</strong></p></div>
    <div class="actions">
      <Button variant="outline" onclick={() => setLanguage('en')}>EN</Button>
      <Button variant="outline" onclick={() => setLanguage('tr')}>TR</Button>
      {#if authenticated}<Button variant="outline" disabled={busy} onclick={() => signOut()}>{t('browser.logout')}</Button>{/if}
    </div>
  </header>
  {#if error}<div role="alert" class="message error">{error}</div>{/if}
  {#if notice}<div role="status" class="message">{notice}</div>{/if}
  {#if initialized && !authenticated}
    <main class="login panel">
      <h2>{t(setup ? 'browser.setup' : 'browser.login')}</h2>
      <p>{t('browser.approval')}</p>
      <form onsubmit={signIn}>
        {#if setup}
          <label for="setup-code">{t('browser.code')}</label>
          <Input id="setup-code" bind:value={code} autocomplete="off" required />
          <p class="muted">{t('browser.password_help')}</p>
        {/if}
        <label for="password">{t('browser.password')}</label>
        <Input id="password" type="password" bind:value={password} minlength={setup ? 12 : 1} maxlength={1024} autocomplete={setup ? 'new-password' : 'current-password'} required />
        <Button type="submit" disabled={busy}>{t(setup ? 'browser.setup' : 'browser.login')}</Button>
      </form>
    </main>
  {:else if authenticated}
    <nav aria-label={t('browser.title')}>
      {#each pages as tab}<Button variant={page === tab ? 'default' : 'outline'} onclick={() => select(tab)}>{t(`browser.${tab}` as TKey)}</Button>{/each}
    </nav>
    <main class="panel">
      <div class="heading"><h2>{t(`browser.${page}` as TKey)}</h2><span role="status">{t(connected ? 'browser.connected' : 'browser.connecting')}</span><Button variant="outline" onclick={refresh}>{t('browser.refresh')}</Button></div>
      {#if page === 'operations'}
        <ServerOperations />
      {:else if page === 'dashboard'}
        {#each servers as server (server.id)}
          <article class="row">
            <div><h3>{server.name}</h3><p>{status(server.status)}</p>{#if server.endpoint}<code>{server.endpoint}</code>{/if}{#if server.failure}<p class="error">{server.failure.code ?? server.failure.message ?? JSON.stringify(server.failure)}</p>{/if}</div>
            <div class="actions"><Button disabled={busy} onclick={() => action(() => control(`/servers/${encodeURIComponent(server.id)}/${stopped(server.status) ? 'start' : 'stop'}`, 'POST'))}>{t(stopped(server.status) ? 'browser.start' : 'browser.stop')}</Button><Button variant="outline" disabled={busy} onclick={() => action(() => control(`/servers/${encodeURIComponent(server.id)}/retry`, 'POST'))}>{t('browser.retry')}</Button></div>
          </article>
        {:else}<p>{t('browser.empty')}</p>{/each}
      {:else if page === 'library'}
        {#if selectedModel}
          <form onsubmit={createAndLoad}>
            <p>{selectedModel.model.name}</p>
            <label for="model-alias">{t('browser.alias')}</label>
            <Input id="model-alias" bind:value={alias} required />
            <div class="actions"><Button type="submit" disabled={busy}>{t('browser.create_load')}</Button><Button variant="outline" onclick={() => selectedModel = null}>{t('browser.cancel')}</Button></div>
          </form>
        {/if}
        {#each definitions as definition (definition.id)}
          <article class="row"><div><h3>{definition.name || definition.alias}</h3><code>{definition.alias}</code></div><Button disabled={busy} onclick={() => action(() => control('/load', 'POST', { definition_id: definition.id }))}>{t('browser.load')}</Button></article>
        {/each}
        {#each models as entry (entry.model.id)}
          <article class="row"><div><h3>{entry.model.name}</h3><p>{bytes(entry.model.size_bytes)}</p><code>{entry.model.path}</code></div><Button disabled={busy} onclick={() => loadModel(entry)}>{t('browser.load')}</Button>{#if !entry.model.source}<Button variant="destructive" disabled={busy} onclick={() => deleteModel(entry.model.id)}>{t('browser.delete')}</Button>{/if}</article>
        {:else}<p>{t('browser.empty')}</p>{/each}
      {:else if page === 'downloads'}
        <form onsubmit={event => { event.preventDefault(); void action(() => control('/models/pull', 'POST', { repo, file })); }}>
          <label for="repo">{t('browser.repo')}</label><Input id="repo" bind:value={repo} placeholder="owner/model-GGUF" required />
          <label for="file">{t('browser.file')}</label><Input id="file" bind:value={file} placeholder="Q4_K_M" required />
          <Button type="submit" disabled={busy}>{t('browser.pull')}</Button>
        </form>
        {#each downloads as download (download.id)}
          <article class="row"><div><h3>{download.repo_id} / {download.variant_name}</h3><p>{status(download.status)} · {bytes(download.downloaded_bytes)} / {bytes(download.total_bytes)}</p><progress max={download.total_bytes || 1} value={download.downloaded_bytes}></progress></div><div class="actions">
            <Button variant="outline" disabled={busy} onclick={() => action(() => control(`/downloads/${download.id}/pause`, 'POST'))}>{t('browser.pause')}</Button>
            <Button variant="outline" disabled={busy} onclick={() => action(() => control(`/downloads/${download.id}/resume`, 'POST'))}>{t('browser.resume')}</Button>
            <Button variant="outline" disabled={busy} onclick={() => action(() => control(`/downloads/${download.id}`, 'DELETE'))}>{t('browser.cancel')}</Button>
          </div></article>
        {/each}
      {:else if page === 'settings'}
        <form onsubmit={event => { event.preventDefault(); void action(async () => { await control('/browser/settings', 'PUT', { default_ctx: context, default_ngl: layers }); notice = t('browser.saved'); }, false); }}>
          <label for="context">{t('browser.context')}</label><Input id="context" type="number" min={0} max={2097152} bind:value={context} required />
          <label for="layers">{t('browser.layers')}</label><Input id="layers" type="number" min={0} max={999} bind:value={layers} required />
          <Button type="submit" disabled={busy}>{t('browser.save')}</Button>
        </form>
        <Button variant="destructive" disabled={busy} onclick={() => signOut(true)}>{t('browser.revoke')}</Button>
      {:else if page === 'tokens'}
        <form onsubmit={event => { event.preventDefault(); void action(async () => { const created = await control<{ token: string }>('/tokens', 'POST', { name: tokenName, scopes }); newToken = created.token; tokenName = ''; }); }}>
          <label for="token-name">{t('browser.name')}</label><Input id="token-name" bind:value={tokenName} required />
          <fieldset><legend>{t('browser.scopes')}</legend><div class="actions">{#each availableScopes as scope}<label class="scope"><input type="checkbox" bind:group={scopes} value={scope} /> {scope}</label>{/each}</div></fieldset>
          <Button type="submit" disabled={busy || scopes.length === 0}>{t('browser.create_token')}</Button>
        </form>
        {#if newToken}<div role="status" class="message"><p>{t('browser.token_once')}</p><code class="secret">{newToken}</code></div>{/if}
        {#each tokens as token (token.id)}<article class="row"><div><h3>{token.name}</h3><p>{token.scopes.join(', ')}</p></div><Button variant="destructive" disabled={busy} onclick={() => action(() => control(`/tokens/${encodeURIComponent(token.id)}`, 'DELETE'))}>{t('browser.revoke_token')}</Button></article>{/each}
      {/if}
    </main>
  {/if}
</div>

<style>
  .browser-shell { max-width: 1160px; margin: auto; padding: 24px; height: 100dvh; overflow: auto; color: var(--foreground); background: var(--background); }
  header, .heading, .row { display: flex; justify-content: space-between; align-items: center; gap: 16px; flex-wrap: wrap; }
  header { margin-bottom: 28px; }
  h1 { font-size: 1.6rem; font-weight: 650; }
  h2 { font-size: 1.2rem; font-weight: 600; }
  h3 { font-weight: 600; }
  p { margin: 8px 0; }
  nav, .actions { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  nav { margin-bottom: 16px; }
  .panel { border: 1px solid var(--border); background: var(--card); border-radius: 12px; padding: 24px; }
  .login { max-width: 540px; margin: 48px auto; }
  form { display: grid; gap: 12px; margin: 20px 0; max-width: 580px; }
  form > :global(button) { justify-self: start; }
  .row { border-top: 1px solid var(--border); padding: 20px 0; }
  .row:first-of-type { margin-top: 20px; }
  .row > div:first-child { min-width: 0; flex: 1 1 220px; }
  code { overflow-wrap: anywhere; font-size: .8rem; }
  .secret { user-select: all; }
  .message { border: 1px solid var(--border); border-radius: 8px; padding: 16px; margin: 12px 0; overflow-wrap: anywhere; }
  .error { color: var(--destructive); }
  .muted { color: var(--muted-foreground); font-size: .875rem; }
  .scope { display: inline-flex; align-items: center; gap: 4px; font-size: .875rem; }
  progress { width: 100%; }
  @media (max-width: 640px) { .browser-shell { padding: 12px; } .panel { padding: 16px; } }
</style>
