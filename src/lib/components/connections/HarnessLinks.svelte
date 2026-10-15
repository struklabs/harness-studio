<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { t } from '#lib/i18n.svelte.js';
  import { errorText } from '#lib/errors.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';

  interface Link {
    id: string;
    name: string;
    base_url: string;
    allow_insecure_http: boolean;
    enabled: boolean;
    token_set: boolean;
    state: 'online' | 'offline' | 'unauthorized';
    model_count: number;
    last_error: string | null;
    last_ok: number | null;
  }

  let links = $state<Link[]>([]);
  let busy = $state(false);
  let error = $state('');
  let open = $state(false);
  let editing = $state<Link | null>(null);
  let removing = $state<Link | null>(null);
  let name = $state('');
  let address = $state('');
  let token = $state('');
  let allowHttp = $state(false);
  let enabled = $state(true);
  let formError = $state('');
  let alive = true;
  const http = $derived(address.trim().toLowerCase().startsWith('http:'));

  async function load() {
    try {
      const result = await invoke<Link[]>('link_list');
      if (alive) { links = result; error = ''; }
    } catch (e) { if (alive) error = errorText(e); }
  }

  onMount(() => {
    void load();
    const timer = setInterval(() => { if (!busy) void load(); }, 10000);
    return () => { alive = false; clearInterval(timer); token = ''; };
  });

  function edit(link: Link | null) {
    editing = link;
    name = link?.name ?? '';
    address = link?.base_url ?? '';
    token = '';
    allowHttp = link?.allow_insecure_http ?? false;
    enabled = link?.enabled ?? true;
    formError = '';
    open = true;
  }

  async function save() {
    busy = true;
    formError = '';
    try {
      const args = { name, baseUrl: address, token: token || null, allowInsecureHttp: http && allowHttp };
      if (editing) await invoke('link_update', { ...args, id: editing.id, enabled });
      else await invoke('link_add', { ...args, token });
      open = false;
      await load();
    } catch (e) { formError = errorText(e); }
    finally { token = ''; busy = false; }
  }

  async function test(link: Link) {
    busy = true;
    error = '';
    try { await invoke('link_test', { id: link.id }); await load(); }
    catch (e) { error = errorText(e); }
    finally { busy = false; }
  }

  async function remove() {
    if (!removing) return;
    busy = true;
    try { await invoke('link_remove', { id: removing.id }); removing = null; await load(); }
    catch (e) { error = errorText(e); }
    finally { busy = false; }
  }
</script>

<div class="flex flex-col gap-4">
  <p class="text-sm text-muted-foreground">{t('link.description')}</p>
  {#if !app.gateway?.status.running}
    <p class="rounded-lg border border-border p-3 text-sm text-muted-foreground">{t('link.gateway_off')}</p>
  {/if}
  <div><Button onclick={() => edit(null)} disabled={busy}>{t('link.add')}</Button></div>
  {#if error}<p class="text-sm text-destructive" role="alert">{error}</p>{/if}
  {#if links.length === 0}<p class="text-sm text-muted-foreground">{t('link.empty')}</p>{/if}
  {#each links as link (link.id)}
    <article class="flex flex-col gap-3 rounded-lg border border-border bg-card p-4">
      <div class="flex flex-wrap items-center gap-2">
        <h2 class="text-sm font-semibold">{link.name}</h2>
        <Badge variant="outline">{t(link.state === 'online' ? 'link.online' : link.state === 'unauthorized' ? 'link.token_rejected' : 'link.offline')}</Badge>
        {#if !link.enabled}<Badge variant="outline">{t('link.disabled')}</Badge>{/if}
        <span class="text-xs text-muted-foreground">{t('link.model_count', { count: link.model_count })}</span>
      </div>
      <p class="break-all font-mono text-xs text-muted-foreground">{link.base_url}</p>
      {#if link.last_error}<p class="text-xs text-destructive">{errorText(link.last_error)}</p>{/if}
      {#if link.last_ok}<p class="text-xs text-muted-foreground">{t('link.last_ok', { time: new Date(link.last_ok * 1000).toLocaleString() })}</p>{/if}
      <div class="flex flex-wrap gap-2">
        <Button variant="outline" size="sm" disabled={busy} onclick={() => test(link)}>{t('link.test')}</Button>
        <Button variant="outline" size="sm" disabled={busy} onclick={() => edit(link)}>{t('link.edit')}</Button>
        <Button variant="outline" size="sm" disabled={busy} onclick={() => { removing = link; }}>{t('link.remove')}</Button>
      </div>
    </article>
  {/each}
</div>

<Dialog.Root bind:open onOpenChange={(value) => { if (!value) token = ''; }}>
  <Dialog.Content class="sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title>{t(editing ? 'link.edit' : 'link.add')}</Dialog.Title>
      <Dialog.Description>{t('link.description')}</Dialog.Description>
    </Dialog.Header>
    <form class="flex flex-col gap-4" onsubmit={(event) => { event.preventDefault(); void save(); }}>
      <label class="flex flex-col gap-1 text-sm">{t('link.name')}<Input bind:value={name} required maxlength={64} disabled={busy} /></label>
      <p class="text-xs text-muted-foreground">{t('link.name_hint')}</p>
      <label class="flex flex-col gap-1 text-sm">{t('link.address')}<Input type="url" bind:value={address} required disabled={busy} placeholder="https://harness.example.com" /></label>
      <label class="flex flex-col gap-1 text-sm">{t('link.token')}<Input type="password" bind:value={token} autocomplete="new-password" required={!editing} disabled={busy} /></label>
      {#if editing}<p class="text-xs text-muted-foreground">{t('link.token_hint')}</p>{/if}
      {#if http}<label class="flex items-center gap-2 text-sm"><input type="checkbox" bind:checked={allowHttp} disabled={busy} />{t('link.allow_http')}</label>{/if}
      {#if editing}<label class="flex items-center gap-2 text-sm"><input type="checkbox" bind:checked={enabled} disabled={busy} />{t('link.enabled')}</label>{/if}
      {#if formError}<p class="text-sm text-destructive" role="alert">{formError}</p>{/if}
      <Dialog.Footer>
        <Button type="button" variant="outline" disabled={busy} onclick={() => { open = false; token = ''; }}>{t('link.cancel')}</Button>
        <Button type="submit" disabled={busy || (http && !allowHttp)}>{t('link.save')}</Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root open={!!removing} onOpenChange={(value) => { if (!value && !busy) removing = null; }}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t('link.remove')}</Dialog.Title>
      <Dialog.Description>{t('link.remove_confirm', { name: removing?.name ?? '' })}</Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" disabled={busy} onclick={() => { removing = null; }}>{t('link.cancel')}</Button>
      <Button variant="destructive" disabled={busy} onclick={remove}>{t('link.remove')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
