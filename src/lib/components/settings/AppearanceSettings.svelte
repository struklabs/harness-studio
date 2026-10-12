<script lang="ts">
  import { onMount, onDestroy, untrack } from 'svelte';
  import { invoke } from '$lib/ipc';
  import { toast } from 'svelte-sonner';
  import type { AppSettings, CloseAction } from '$lib/types';
  import { applyTheme, baseMode, schemeOf, themeFromSettings, type ThemeId } from '$lib/theme';
  import Check from '@lucide/svelte/icons/check';
  import { applyScale, normalizeScale } from '$lib/uiScale';
  import { setLanguage, getLanguage, t, type Language } from '$lib/i18n.svelte';
  import * as ToggleGroup from '$lib/components/ui/toggle-group';
  import { cn } from '$lib/utils';
  import type { SettingsDraft } from './form.svelte';
  import { app } from '$lib/stores/app.svelte';
  import UiLevelToggle from '../common/UiLevelToggle.svelte';
  import SettingGroup from './SettingGroup.svelte';
  import SettingRow from './SettingRow.svelte';
  import SwitchRow from './SwitchRow.svelte';

  interface Props {
    settings: AppSettings;
    form: SettingsDraft;
    onSettingsUpdated: (updated: AppSettings) => void;
  }

  let { settings, form, onSettingsUpdated }: Props = $props();

  const detach = <T,>(v: T): T => structuredClone($state.snapshot(v) as T);

  // Theme and language apply immediately and are saved on their own, on top of the *latest* saved
  // settings, so unrelated unsaved edits elsewhere on the page are not written along with them.
  let theme = $state<ThemeId>(untrack(() => themeFromSettings(settings)));
  let cvd = $state(untrack(() => !!settings.cvd_safe));
  let lang = $state<Language>(untrack(() => settings.language ?? 'tr'));
  $effect(() => {
    theme = themeFromSettings(settings);
    cvd = !!settings.cvd_safe;
    lang = settings.language ?? 'tr';
  });

  async function savePreference(mutate: (s: AppSettings) => void) {
    const next = detach(app.settings ?? settings);
    mutate(next);
    await invoke('save_app_settings', { settings: next });
    onSettingsUpdated(next);
  }

  async function changeTheme(next: ThemeId) {
    const prev = theme;
    if (next === prev) return;
    theme = next;
    applyTheme(next, cvd);
    try {
      await savePreference((s) => {
        s.theme = baseMode(next);
        s.color_theme = next;
      });
    } catch (e) {
      theme = prev;
      applyTheme(prev, cvd);
      toast.error(t('settings.theme_save_failed', { error: String(e) }));
    }
  }

  async function changeCvd(next: boolean) {
    const prev = cvd;
    if (next === prev) return;
    cvd = next;
    applyTheme(theme, next);
    try {
      await savePreference((s) => (s.cvd_safe = next));
    } catch (e) {
      cvd = prev;
      applyTheme(theme, prev);
      toast.error(t('settings.cvd_save_failed', { error: String(e) }));
    }
  }

  // OS accessibility modes, noted under the theme grid (they change what you see).
  let forcedColors = $state(false);
  let contrastMore = $state(false);
  onMount(() => {
    const fc = window.matchMedia('(forced-colors: active)');
    const cm = window.matchMedia('(prefers-contrast: more)');
    const sync = () => {
      forcedColors = fc.matches;
      contrastMore = cm.matches;
    };
    sync();
    fc.addEventListener('change', sync);
    cm.addEventListener('change', sync);
    return () => {
      fc.removeEventListener('change', sync);
      cm.removeEventListener('change', sync);
    };
  });

  async function changeLanguage(next: Language) {
    const prev = getLanguage();
    if (next === lang && next === prev) return;
    lang = next;
    setLanguage(next);
    try {
      await savePreference((s) => (s.language = next));
    } catch (e) {
      lang = prev;
      setLanguage(prev);
      toast.error(t('settings.lang_save_failed', { error: String(e) }));
    }
  }

  let scale = $derived(normalizeScale(settings.ui_scale));
  async function changeScale(next: number) {
    const prev = scale;
    if (next === prev) return;
    void applyScale(next);
    try {
      await savePreference((s) => (s.ui_scale = next));
    } catch (e) {
      void applyScale(prev);
      toast.error(t('settings.ui_scale_save_failed', { error: String(e) }));
    }
  }

  const scales: { id: number; label: () => string }[] = [
    { id: 90, label: () => t('settings.ui_scale_small') },
    { id: 100, label: () => t('settings.ui_scale_normal') },
    { id: 115, label: () => t('settings.ui_scale_medium_large') },
    { id: 130, label: () => t('settings.ui_scale_large') },
    { id: 150, label: () => t('settings.ui_scale_xlarge') },
  ];

  // Each theme exists for a reason; the one-line description says which.
  const themes: { id: ThemeId; label: () => string; desc: () => string }[] = [
    { id: 'system', label: () => t('settings.theme_system_option'), desc: () => t('settings.theme_desc_system') },
    { id: 'dark', label: () => t('settings.theme_dark_option'), desc: () => t('settings.theme_desc_dark') },
    { id: 'light', label: () => t('settings.theme_light_option'), desc: () => t('settings.theme_desc_light') },
    { id: 'oled', label: () => t('settings.theme_oled_option'), desc: () => t('settings.theme_desc_oled') },
    { id: 'night', label: () => t('settings.theme_night_option'), desc: () => t('settings.theme_desc_night') },
    { id: 'hc-dark', label: () => t('settings.theme_hc_dark_option'), desc: () => t('settings.theme_desc_hc_dark') },
    { id: 'hc-light', label: () => t('settings.theme_hc_light_option'), desc: () => t('settings.theme_desc_hc_light') },
  ];

  // --- Launch at login: effective OS state, applied immediately ---------------
  let launchAtLogin = $state<boolean | null>(null);
  let launchBusy = $state(false);
  let launchError = $state<string | null>(null);
  let destroyed = false;

  // Without a tray icon (some Linux desktops) closing always quits: say so next to the choice.
  let trayAvailable = $state(true);
  invoke<boolean>('is_tray_available')
    .then((v) => {
      if (!destroyed) trayAvailable = v;
    })
    .catch(() => {});

  async function loadLaunchAtLogin() {
    try {
      const v = await invoke<boolean>('get_launch_at_login');
      if (!destroyed) launchAtLogin = v;
    } catch {
      if (!destroyed) launchAtLogin = false;
    }
  }

  async function toggleLaunchAtLogin(enabled: boolean) {
    if (launchBusy) return;
    launchBusy = true;
    launchError = null;
    try {
      launchAtLogin = await invoke<boolean>('set_launch_at_login', { enabled });
    } catch (e) {
      launchError = t('settings.launch_at_login_failed', { error: String(e) });
      await loadLaunchAtLogin();
    } finally {
      launchBusy = false;
    }
  }

  onMount(loadLaunchAtLogin);
  onDestroy(() => (destroyed = true));
</script>

{#snippet themePreview(id: Exclude<ThemeId, 'system'>)}
  <!-- Rendered with the theme's own tokens: the swatch is the real palette (and CVD variant). -->
  <div
    data-theme={id}
    data-scheme={schemeOf(id)}
    data-cvd={cvd ? 'on' : undefined}
    class="keep-colors flex h-full min-w-0 flex-1 gap-1 bg-background p-1.5"
  >
    <div class="flex w-1/4 flex-col gap-1 rounded-sm border border-sidebar-border bg-sidebar p-1">
      <span class="h-1 rounded-full bg-primary"></span>
      <span class="h-1 rounded-full bg-sidebar-foreground/50"></span>
      <span class="h-1 rounded-full bg-sidebar-foreground/50"></span>
    </div>
    <div class="flex min-w-0 flex-1 flex-col justify-between rounded-sm border border-border bg-card p-1">
      <span class="h-1 w-2/3 rounded-full bg-foreground"></span>
      <span class="h-1 w-1/2 rounded-full bg-muted-foreground"></span>
      <span class="flex gap-0.5">
        <span class="size-1.5 rounded-full bg-success"></span>
        <span class="size-1.5 rounded-full bg-warning"></span>
        <span class="size-1.5 rounded-full bg-destructive"></span>
        <span class="size-1.5 rounded-full bg-info"></span>
      </span>
    </div>
  </div>
{/snippet}

<div class="flex flex-col gap-6">
  <SettingGroup title={t('settings.group_look')}>
    <SettingRow label={t('uiLevel.label')} hint={t('uiLevel.hint')}><UiLevelToggle /></SettingRow>
    <div class="flex flex-col gap-2 px-4 py-3">
      <span id="theme-label" class="text-sm font-medium">{t('settings.theme')}</span>
      <div class="grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-3 max-[1100px]:gap-2" role="radiogroup" aria-labelledby="theme-label">
        {#each themes as opt (opt.id)}
          {@const selected = theme === opt.id}
          <label
            class={cn(
              'relative flex min-w-0 cursor-pointer flex-col gap-1.5 rounded-md border p-1.5 transition-colors has-[:focus-visible]:outline-solid has-[:focus-visible]:outline-(length:--focus-width) has-[:focus-visible]:outline-offset-2 has-[:focus-visible]:outline-ring',
              selected ? 'border-primary bg-primary/5 ring-1 ring-primary' : 'border-border hover:border-foreground/30',
            )}
          >
            <input
              type="radio"
              name="app-theme"
              class="sr-only"
              value={opt.id}
              checked={selected}
              aria-describedby="theme-desc-{opt.id}"
              onchange={() => changeTheme(opt.id)}
            />
            <span class="flex h-14 overflow-hidden rounded-sm border border-border" aria-hidden="true">
              {#if opt.id === 'system'}
                {@render themePreview(contrastMore ? 'hc-dark' : 'dark')}
                {@render themePreview(contrastMore ? 'hc-light' : 'light')}
              {:else}
                {@render themePreview(opt.id)}
              {/if}
            </span>
            <span class="flex min-w-0 items-center gap-1 text-xs font-medium text-foreground">
              <span class="truncate">{opt.label()}</span>
              {#if selected}<Check class="size-3.5 shrink-0 text-primary" aria-hidden="true" />{/if}
            </span>
            <span id="theme-desc-{opt.id}" class="text-xs leading-snug text-muted-foreground">{opt.desc()}</span>
          </label>
        {/each}
      </div>
      {#if forcedColors}
        <p class="text-xs text-muted-foreground" role="note">{t('settings.theme_forced_colors')}</p>
      {:else if contrastMore && theme !== 'system' && !theme.startsWith('hc-')}
        <p class="text-xs text-muted-foreground" role="note">{t('settings.theme_contrast_more')}</p>
      {/if}
    </div>

    <SwitchRow id="cvd-safe" label={t('settings.cvd_label')} hint={t('settings.cvd_desc')} checked={cvd} onchange={changeCvd} />

    <SettingRow label={t('settings.ui_scale')} hint={t('settings.ui_scale_hint')}>
      <ToggleGroup.Root
        type="single"
        variant="outline"
        size="sm"
        aria-label={t('settings.ui_scale')}
        bind:value={() => String(scale), (v) => v && changeScale(Number(v))}
      >
        {#each scales as s (s.id)}
          <ToggleGroup.Item value={String(s.id)} aria-label={s.label()} title={s.label()}>{s.id}%</ToggleGroup.Item>
        {/each}
      </ToggleGroup.Root>
    </SettingRow>

    <SettingRow label={t('settings.language')} hint={t('settings.language_hint')}>
      <ToggleGroup.Root
        type="single"
        variant="outline"
        size="sm"
        aria-label={t('settings.language')}
        bind:value={() => lang, (v) => v && changeLanguage(v as Language)}
      >
        <ToggleGroup.Item value="tr" aria-label="Türkçe">Türkçe</ToggleGroup.Item>
        <ToggleGroup.Item value="en" aria-label="English">English</ToggleGroup.Item>
      </ToggleGroup.Root>
    </SettingRow>
  </SettingGroup>

  <SettingGroup title={t('settings.startup_title')} hint={t('settings.startup_desc')}>
    <SwitchRow
      id="launch-at-login"
      label={t('settings.launch_at_login_label')}
      hint={t('settings.launch_at_login_desc')}
      checked={launchAtLogin === true}
      disabled={launchAtLogin === null || launchBusy}
      onchange={toggleLaunchAtLogin}
    />
    {#if launchError}
      <p class="px-4 py-2 text-xs break-words text-destructive" role="alert">{launchError}</p>
    {/if}
    <SwitchRow
      id="start-minimized"
      label={t('settings.start_minimized_label')}
      hint={t('settings.start_minimized_desc')}
      note={launchAtLogin !== true ? t('settings.start_minimized_requires_login') : undefined}
      checked={form.get('start_minimized')}
      dirty={form.isDirty('start_minimized')}
      disabled={launchAtLogin !== true}
      onchange={(v) => form.set('start_minimized', v)}
    />
    <SettingRow
      label={t('settings.close_action_label')}
      help={t('settings.close_action_help')}
      note={!trayAvailable
        ? t('settings.close_action_no_tray')
        : form.get('close_action') === 'tray'
          ? t('settings.close_action_desc_tray')
          : form.get('close_action') === 'quit'
            ? t('settings.close_action_desc_quit')
            : t('settings.close_action_desc_ask')}
      dirty={form.isDirty('close_action')}
    >
      <ToggleGroup.Root
        type="single"
        variant="outline"
        size="sm"
        value={form.get('close_action')}
        onValueChange={(v) => {
          if (v) form.set('close_action', v as CloseAction);
        }}
        aria-label={t('settings.close_action_label')}
      >
        <ToggleGroup.Item value="ask">{t('settings.close_action_ask')}</ToggleGroup.Item>
        <ToggleGroup.Item value="tray" disabled={!trayAvailable}>{t('settings.close_action_tray')}</ToggleGroup.Item>
        <ToggleGroup.Item value="quit">{t('settings.close_action_quit')}</ToggleGroup.Item>
      </ToggleGroup.Root>
    </SettingRow>
    <SwitchRow
      id="keep-window-in-tray"
      label={t('settings.keep_window_in_tray_label')}
      hint={t('settings.keep_window_in_tray_desc')}
      note={!trayAvailable ? t('settings.close_action_no_tray') : undefined}
      checked={form.get('keep_window_in_tray')}
      dirty={form.isDirty('keep_window_in_tray')}
      disabled={!trayAvailable}
      onchange={(v) => form.set('keep_window_in_tray', v)}
    />
    <SwitchRow
      id="restore-last-session"
      label={t('settings.restore_last_session_label')}
      hint={t('settings.restore_last_session_desc')}
      checked={form.get('restore_last_session')}
      dirty={form.isDirty('restore_last_session')}
      onchange={(v) => form.set('restore_last_session', v)}
    />
  </SettingGroup>
</div>
