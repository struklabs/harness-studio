<script lang="ts">
  import { Switch } from '#lib/components/ui/switch/index.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { HarnessError, THEMES, UI_SCALES, appSettings, can, updateSettings, type HarnessConnection, type Identity, type RemoteSettings } from '../harness.ts';
  import { errorText } from '../text.ts';

  // The computer's app settings that the Control API allows (appearance, window, downloads,
  // request analytics). Any token with `read` sees them; only an admin token changes them.
  let { connection, identity }: { connection: HarnessConnection; identity: Identity | null } = $props();

  let values = $state<RemoteSettings | null>(null);
  let error = $state<string | null>(null);
  let saving = $state<keyof RemoteSettings | null>(null);
  const editable = $derived(can(identity, 'admin'));

  $effect(() => {
    const c = connection;
    values = null;
    appSettings(c)
      .then((v) => {
        values = v;
        error = null;
      })
      .catch((e) => (error = e instanceof HarnessError ? e.key : 'mobile.transport'));
  });

  async function change<K extends keyof RemoteSettings>(key: K, value: RemoteSettings[K]) {
    if (!values || values[key] === value) return;
    saving = key;
    error = null;
    try {
      values = await updateSettings(connection, { [key]: value } as Partial<RemoteSettings>);
    } catch (e) {
      error = e instanceof HarnessError ? e.key : 'mobile.transport';
    } finally {
      saving = null;
    }
  }

  const selectClass = 'h-8 rounded-md border border-input bg-background px-2 text-sm disabled:opacity-60';
  const toggles: { key: keyof RemoteSettings; label: TKey }[] = [
    { key: 'cvd_safe', label: 'mobile.settings.cvd_safe' },
    { key: 'start_minimized', label: 'mobile.settings.start_minimized' },
    { key: 'restore_last_session', label: 'mobile.settings.restore_last_session' },
    { key: 'keep_window_in_tray', label: 'mobile.settings.keep_window_in_tray' },
    { key: 'auto_load_after_download', label: 'mobile.settings.auto_load' },
    { key: 'analytics_enabled', label: 'mobile.settings.analytics' },
  ];
</script>

<section class="space-y-2" data-testid="app-settings">
  <h2 class="text-sm font-semibold">{t('mobile.manage.settings')}</h2>
  <p class="text-xs text-muted-foreground">{editable ? t('mobile.settings.hint_admin') : t('mobile.settings.hint_read')}</p>
  {#if error}<p class="text-xs text-destructive" role="alert">{errorText(error)}</p>{/if}
  {#if values}
    <div class="divide-y divide-border rounded-lg border border-border bg-card text-sm">
      <label class="flex items-center justify-between gap-2 p-2">
        <span>{t('mobile.settings.language')}</span>
        <select class={selectClass} disabled={!editable || saving !== null} value={values.language} onchange={(e) => change('language', e.currentTarget.value as RemoteSettings['language'])}>
          <option value="tr">Türkçe</option>
          <option value="en">English</option>
        </select>
      </label>
      <label class="flex items-center justify-between gap-2 p-2">
        <span>{t('mobile.settings.theme')}</span>
        <select class={selectClass} disabled={!editable || saving !== null} value={values.theme} onchange={(e) => change('theme', e.currentTarget.value)}>
          {#each THEMES as id (id)}<option value={id}>{t(`mobile.settings.theme.${id}` as TKey)}</option>{/each}
        </select>
      </label>
      <label class="flex items-center justify-between gap-2 p-2">
        <span>{t('mobile.settings.ui_scale')}</span>
        <select class={selectClass} disabled={!editable || saving !== null} value={values.ui_scale ?? ''} onchange={(e) => change('ui_scale', Number(e.currentTarget.value))}>
          {#if values.ui_scale === null}<option value="">{t('mobile.settings.automatic')}</option>{/if}
          {#each UI_SCALES as scale (scale)}<option value={scale}>{scale}%</option>{/each}
        </select>
      </label>
      <label class="flex items-center justify-between gap-2 p-2">
        <span>{t('mobile.settings.ui_level')}</span>
        <select class={selectClass} disabled={!editable || saving !== null} value={values.ui_level ?? ''} onchange={(e) => change('ui_level', e.currentTarget.value as 'simple' | 'advanced')}>
          {#if values.ui_level === null}<option value="">{t('mobile.settings.ui_level_full')}</option>{/if}
          <option value="simple">{t('mobile.settings.ui_level_simple')}</option>
          <option value="advanced">{t('mobile.settings.ui_level_advanced')}</option>
        </select>
      </label>
      <label class="flex items-center justify-between gap-2 p-2">
        <span>{t('mobile.settings.close_action')}</span>
        <select class={selectClass} disabled={!editable || saving !== null} value={values.close_action} onchange={(e) => change('close_action', e.currentTarget.value as RemoteSettings['close_action'])}>
          <option value="ask">{t('mobile.settings.close_ask')}</option>
          <option value="tray">{t('mobile.settings.close_tray')}</option>
          <option value="quit">{t('mobile.settings.close_quit')}</option>
        </select>
      </label>
      {#each toggles as item (item.key)}
        <div class="flex items-center justify-between gap-2 p-2">
          <span id="label-{item.key}">{t(item.label)}</span>
          <Switch aria-labelledby="label-{item.key}" checked={values[item.key] as boolean} disabled={!editable || saving !== null} onCheckedChange={(v: boolean) => change(item.key, v as never)} />
        </div>
      {/each}
      <label class="flex items-center justify-between gap-2 p-2">
        <span>{t('mobile.settings.retention')}</span>
        <input
          type="number"
          min="1"
          max="3650"
          class="{selectClass} w-20 text-right"
          disabled={!editable || saving !== null}
          value={values.analytics_retention_days}
          onchange={(e) => change('analytics_retention_days', Number(e.currentTarget.value))}
        />
      </label>
    </div>
  {:else if !error}
    <p class="text-xs text-muted-foreground">{t('mobile.loading')}</p>
  {/if}
</section>
