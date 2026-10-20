<script lang="ts">
  import { onMount } from 'svelte';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { control, ControlError } from '#lib/transport.js';

  let service = $state<{ managed: boolean; uptime_seconds: number; start_on_boot: boolean; version: string; restore_failed?: boolean }>();
  let update = $state<{ current: string; version: string; available: boolean; applicable: boolean }>();
  let logs = $state('');
  let password = $state('');
  let archive = $state<File>();
  let busy = $state(false);
  let message = $state('');
  let error = $state('');
  async function run(fn: () => Promise<void>) {
    if (busy) return;
    busy = true; error = ''; message = '';
    try { await fn(); }
    catch (e) { error = e instanceof ControlError ? t(e.code as TKey, e.params) : String(e); }
    finally { busy = false; password = ''; }
  }
  async function refresh() { service = await control('/browser/service'); }
  function download(value: string, name: string, type: string) {
    const url = URL.createObjectURL(new Blob([value], { type }));
    const link = document.createElement('a'); link.href = url; link.download = name; link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  async function lifecycle(action: 'restart' | 'enable' | 'disable') {
    if (action === 'restart' && !window.confirm(t('browser.restart_confirm'))) return;
    await run(async () => { await control('/browser/service', 'POST', action); message = t('browser.operation_accepted'); if (action !== 'restart') await refresh(); });
  }
  async function restore() {
    if (!archive || !window.confirm(t('browser.restore_confirm'))) return;
    const file = archive;
    await run(async () => {
      if (file.size > 2 * 1024 * 1024) throw new Error(t('browser.invalid_backup'));
      await control('/browser/restore', 'POST', { password, archive: JSON.parse(await file.text()) });
      message = t('browser.restore_staged');
    });
  }
  onMount(() => { void run(refresh); });
</script>

{#if error}<p role="alert">{error}</p>{/if}
{#if message}<p role="status">{message}</p>{/if}
<section>
  <h3>{t('browser.service')}</h3>
  {#if service}
    <p>{t('browser.version')}: {service.version} · {t('browser.uptime')}: {service.uptime_seconds}s</p>
    {#if service.restore_failed}<p role="alert">{t('browser.restore_failed')}</p>{/if}
    <p>{t(service.managed ? 'browser.managed_service' : 'browser.unmanaged_service')}</p>
    <div class="actions">
      <Button disabled={busy} onclick={() => run(refresh)}>{t('browser.refresh')}</Button>
      <Button disabled={busy || !service.managed} onclick={() => lifecycle('restart')}>{t('browser.restart')}</Button>
      <Button disabled={busy || !service.managed} onclick={() => lifecycle(service?.start_on_boot ? 'disable' : 'enable')}>{t(service.start_on_boot ? 'browser.disable_boot' : 'browser.enable_boot')}</Button>
    </div>
  {/if}
</section>
<section>
  <h3>{t('browser.updates')}</h3>
  <Button disabled={busy} onclick={() => run(async () => { update = await control('/browser/update'); })}>{t('browser.check_update')}</Button>
  {#if update}
    <p>{t('browser.version')}: {update.current} → {update.version}</p>
    <p>{t(update.available ? (update.applicable ? 'browser.update_ready' : 'browser.update_manual') : 'browser.update_current')}</p>
    <Button disabled={busy || !update.applicable} onclick={() => { if (window.confirm(t('browser.update_confirm'))) void run(async () => { await control('/browser/update', 'POST', { version: update?.version }); message = t('browser.operation_accepted'); }); }}>{t('browser.apply_update')}</Button>
  {/if}
</section>
<section>
  <h3>{t('browser.logs')}</h3>
  <p>{t('browser.logs_help')}</p>
  <div class="actions">
    <Button disabled={busy || !service?.managed} onclick={() => run(async () => { logs = (await control<{ text: string }>('/browser/logs')).text; })}>{t('browser.view_logs')}</Button>
    <Button disabled={!logs} onclick={() => download(logs, 'harness-server.log', 'text/plain')}>{t('browser.download_logs')}</Button>
  </div>
  {#if logs}<pre>{logs}</pre>{/if}
</section>
<section>
  <h3>{t('browser.backup')}</h3>
  <p>{t('browser.backup_help')}</p>
  <label for="backup-password">{t('browser.backup_password')}</label>
  <Input id="backup-password" type="password" autocomplete="new-password" bind:value={password} minlength={12} maxlength={1024} />
  <div class="actions">
    <Button disabled={busy || password.length < 12} onclick={() => run(async () => { const data = await control('/browser/backup', 'POST', { password }); download(JSON.stringify(data), 'harness-backup.json', 'application/json'); })}>{t('browser.download_backup')}</Button>
    <label for="restore-file">{t('browser.restore_file')}</label>
    <input id="restore-file" type="file" accept=".json,application/json" onchange={event => archive = event.currentTarget.files?.[0]} />
    <Button disabled={busy || !archive || password.length < 12} onclick={restore}>{t('browser.restore')}</Button>
  </div>
</section>

<style>
  section { border-top: 1px solid var(--border); margin-top: 20px; padding-top: 16px; }
  h3 { font-weight: 600; } p { margin: 10px 0; }
  .actions { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; margin-top: 12px; }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; max-height: 420px; overflow: auto; margin-top: 12px; font-size: .8rem; }
</style>
