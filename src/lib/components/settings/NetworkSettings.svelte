<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { localizeError } from '#lib/network.js';
  import type { AppSettings } from '#lib/types.js';
  import { t } from '#lib/i18n.svelte.js';
  import type { SettingsDraft } from './form.svelte';
  import SettingGroup from './SettingGroup.svelte';
  import SettingRow from './SettingRow.svelte';
  import NumberField from './NumberField.svelte';
  import SwitchRow from './SwitchRow.svelte';

  interface Props {
    page?: string;
    settings: AppSettings;
    form: SettingsDraft;
    onSettingsUpdated: (updated: AppSettings) => void;
  }

  let { page = 'network-listen', settings, form, onSettingsUpdated }: Props = $props();
  let remoteAvailable = $state(false);
  let remoteEnabled = $state(false);
  let remoteBusy = $state(false);
  let remoteError = $state('');
  onMount(() => {
    void Promise.all([invoke<boolean>('remote_access_available'), invoke<{enabled: boolean}>('remote_access_settings_get')])
      .then(([available, saved]) => { remoteAvailable = available; remoteEnabled = saved.enabled; })
      .catch((e: unknown) => { remoteError = localizeError(e); });
  });
  async function setRemote(enabled: boolean) {
    remoteBusy = true; remoteError = '';
    try { await invoke('remote_access_settings_set', { enabled }); remoteEnabled = enabled; }
    catch (e) { remoteError = localizeError(e); }
    finally { remoteBusy = false; }
  }
</script>

<div hidden={page !== 'network-listen'} class="flex flex-col gap-6">
  {#if remoteAvailable}
    <SettingGroup advanced title={t('settings.remote_access_title')}>
      <SwitchRow advanced id="remote-access-struklabs" label={t('settings.remote_access_title')} hint={t('settings.remote_access_hint')} note={t('settings.remote_access_preview')} checked={remoteEnabled} disabled={remoteBusy} onchange={setRemote} />
      {#if remoteError}<p role="alert" class="text-sm text-destructive">{remoteError}</p>{/if}
    </SettingGroup>
  {/if}
  <SettingGroup title={t('settings.default_params_title')} hint={t('settings.default_params_desc')}>
    <SettingRow label={t('settings.default_port_label')} for="def-port" hint={t('settings.default_port_hint')} dirty={form.isDirty('port')}>
      <NumberField id="def-port" min={0} max={65535} value={form.get('port')} onchange={(v) => form.set('port', v)} />
    </SettingRow>
    <SettingRow label={t('settings.default_gpu_layers_label')} for="def-ngl" hint={t('settings.default_gpu_layers_hint')} dirty={form.isDirty('default_ngl')}>
      <NumberField id="def-ngl" value={form.get('default_ngl')} onchange={(v) => form.set('default_ngl', v)} />
    </SettingRow>
    <SettingRow label={t('settings.default_ctx_label')} for="def-ctx" hint={t('settings.default_ctx_hint')} dirty={form.isDirty('default_ctx')}>
      <NumberField id="def-ctx" min={512} autoLabel={t('settings.default_ctx_auto')} value={form.get('default_ctx')} onchange={(v) => form.set('default_ctx', v)} />
    </SettingRow>
  </SettingGroup>

  <SettingGroup title={t('settings.group_server_start')}>
    <SettingRow label={t('settings.load_timeout')} for="load-timeout" hint={t('settings.load_timeout_desc')} dirty={form.isDirty('load_timeout_secs')}>
      <NumberField
        id="load-timeout"
        min={10}
        value={form.get('load_timeout_secs')}
        onchange={(v) => form.set('load_timeout_secs', Math.min(7200, Math.max(10, Math.round(v))))}
      />
    </SettingRow>
    <SwitchRow
      id="auto-restart"
      label={t('settings.auto_restart')}
      hint={t('settings.auto_restart_desc')}
      checked={form.get('auto_restart')}
      dirty={form.isDirty('auto_restart')}
      onchange={(v) => form.set('auto_restart', v)}
    />
    {#if form.get('auto_restart')}
      <SettingRow label={t('settings.restart_max_attempts')} for="restart-max" hint={t('settings.restart_max_attempts_desc')} dirty={form.isDirty('restart_max_attempts')}>
        <NumberField
          id="restart-max"
          min={1}
          value={form.get('restart_max_attempts')}
          onchange={(v) => form.set('restart_max_attempts', Math.min(10, Math.max(1, Math.round(v))))}
        />
      </SettingRow>
    {/if}
    <SwitchRow
      id="auto-benchmark"
      label={t('settings.auto_benchmark')}
      hint={t('settings.auto_benchmark_desc')}
      checked={form.get('auto_benchmark')}
      dirty={form.isDirty('auto_benchmark')}
      onchange={(v) => form.set('auto_benchmark', v)}
    />
  </SettingGroup>


</div>
