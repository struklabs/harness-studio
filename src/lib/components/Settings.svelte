<script lang="ts">
  import './common/polished-pages.css';
  import HelpPopover from './common/HelpPopover.svelte';
  import { app } from '#lib/stores/app.svelte.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { tick, untrack } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { toast } from 'svelte-sonner';
  import { Check, TriangleAlert } from '@lucide/svelte';
  import type { AppSettings, SystemHardwareInfo } from '#lib/types.js';
  import { t, inAllLanguages } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { cn } from '#lib/utils.js';
  import { SettingsDraft } from './settings/form.svelte';
  import RuntimeSettings from './settings/RuntimeSettings.svelte';
  import StorageSettings from './settings/StorageSettings.svelte';
  import NetworkSettings from './settings/NetworkSettings.svelte';
  import AppearanceSettings from './settings/AppearanceSettings.svelte';
  import UpdatesSettings from './settings/UpdatesSettings.svelte';
  import PrivacySettings from './settings/PrivacySettings.svelte';
  import SecuritySettings from './settings/SecuritySettings.svelte';
  import DeveloperApiSettings from './settings/DeveloperApiSettings.svelte';
  import OtelSettings from './settings/OtelSettings.svelte';
  import AutomationsSettings from './settings/AutomationsSettings.svelte';
  import GatewaySettings from './settings/GatewaySettings.svelte';
  import ToolsSettings from './settings/ToolsSettings.svelte';
  import DiagnosticsSettings from './settings/DiagnosticsSettings.svelte';
  import { SETTINGS_SECTIONS, SETTINGS_GROUPS, settingsPage } from './settings/sections';
  import { security } from '#lib/stores/security.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { panes } from '#lib/stores/panes.svelte.js';
  import { SectionSelection } from '#lib/stores/sectionPage.svelte.js';
  import AdvancedPageNotice from './common/AdvancedPageNotice.svelte';
  import { pageLevel } from '#lib/uiLevel.js';
  import SectionSidebar from './common/SectionSidebar.svelte';
  import { SETTINGS_SEARCH_KEYS } from './settings/searchFields';
  import { revealSettingsField } from './settings/searchTarget';
  import { holdReleaseWhile } from '#lib/windowActivity.js';

  interface Props {
    settings: AppSettings;
    hardware: SystemHardwareInfo | null;
    onSettingsUpdated: (updated: AppSettings) => void;
    onReopenWizard?: () => void;
  }

  let { settings, hardware, onSettingsUpdated, onReopenWizard }: Props = $props();

  const sections = SETTINGS_SECTIONS;

  const section = new SectionSelection('settings', sections.map(s => s.id), 'runtime-installed', settingsPage, SETTINGS_GROUPS);
  let active = $derived(section.active);
  let activeRoot = $derived(sections.find(s => s.id === active)!.root);
  // Sections mount on first visit and stay mounted, so in-flight downloads and checks survive
  // switching sections; their timers are cleared when they (or this page) are destroyed.
  let visited = $state<Record<string, boolean>>({ runtime: true });
  const activeSection = $derived(sections.find((s) => s.id === active)!);
  let content: HTMLDivElement;
  let settingsSearch = $state<typeof security.requestedSettingsSearch>(null);
  let revealedSearch: typeof security.requestedSettingsSearch = null;

  const navPane = panes.settingsNav;

  function select(id: string) {
    section.select(settingsPage(id));
  }
  $effect(() => {
    visited[activeRoot] = true;
    if (activeRoot === 'network') visited.security = true;
    if (active === 'runtime-ollama') visited.gateway = true;
    if (active === 'diagnostics-reports') visited.privacy = true;
  });
  $effect(() => {
    const requested = security.requestedSection;
    if (requested && sections.some(s => s.id === settingsPage(requested))) {
      select(requested);
      settingsSearch = security.requestedSettingsSearch;
      security.requestedSettingsSearch = null;
      security.requestedSection = null;
    }
  });

  $effect(() => {
    const request = settingsSearch;
    if (!request || request === revealedSearch || active !== settingsPage(request.section) || !content) return;
    const root = content;
    const labels = untrack(() => {
      const sectionLabels = inAllLanguages(activeSection.label);
      return [...sectionLabels, ...inAllLanguages('nav.settings').flatMap((label) =>
        sectionLabels.map((sectionLabel) => `${label}: ${sectionLabel}`))];
    });
    const keys = SETTINGS_SEARCH_KEYS[active] ?? [];
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    let cleanup = () => {};
    void tick().then(() => {
      if (cancelled) return;
      // Let the palette's closing dialog restore focus before focusing the settings field.
      timer = setTimeout(() => {
        if (cancelled) return;
        revealedSearch = request;
        cleanup = revealSettingsField(root, request.query, labels, keys);
      }, 150);
    });
    return () => {
      cancelled = true;
      clearTimeout(timer);
      cleanup();
    };
  });

  // Edits are tracked as a patch on top of the latest saved settings (see settings/form.svelte.ts).
  const form = new SettingsDraft(() => app.settings ?? settings);
  const dirtyCount = $derived(form.dirtyCount);
  // Unsaved edits would be lost with the page while hidden to the tray.
  $effect(() => holdReleaseWhile('settings-form', () => dirtyCount > 0));

  let saving = $state(false);
  let saveError = $state<string | null>(null);

  async function saveSettings() {
    if (saving || uiLevel.busy || dirtyCount === 0) return;
    saving = true;
    saveError = null;
    try {
      const next = form.merged();
      await invoke('save_app_settings', { settings: next });
      onSettingsUpdated(next);
      form.reset();
      toast.success(t('settings.save_success'));
    } catch (e) {
      saveError = t('settings.save_failed', { error: localizeError(e) });
    } finally {
      saving = false;
    }
  }

  function discardChanges() {
    form.reset();
    saveError = null;
  }
</script>

<div class="polished-page flex h-full min-h-0 w-full overflow-hidden bg-background">
  <SectionSidebar pane={navPane} label={t('pane.settings_nav')} groups={SETTINGS_GROUPS} {active} onSelect={select} />

  <main class="flex min-w-0 flex-1 flex-col">
    <div class="min-h-0 flex-1 overflow-y-auto">
      <div bind:this={content} class="flex w-full max-w-5xl flex-col gap-6 px-6 py-6 max-md:px-4">
        <AdvancedPageNotice level={pageLevel(SETTINGS_GROUPS, active)} />
        <header class="flex flex-col gap-1">
          <h1 class="flex items-center gap-1.5 text-lg font-semibold">{t(activeSection.label)}<HelpPopover text={t(activeSection.desc)} label={t(activeSection.label)} /></h1>
        </header>

        <div class:hidden={activeRoot !== 'runtime'}>
          {#if visited.runtime}
            <RuntimeSettings page={active} {settings} {hardware} {form} {onSettingsUpdated} {onReopenWizard} />
          {/if}
        </div>
        <div class:hidden={activeRoot !== 'storage'}>
          {#if visited.storage}
            <StorageSettings page={active} {settings} {form} {onSettingsUpdated} />
          {/if}
        </div>
        <div class:hidden={activeRoot !== 'network'}>
          {#if visited.network}
            <NetworkSettings page={active} {settings} {form} {onSettingsUpdated} />
          {/if}
        </div>
        <div class:hidden={!['security', 'network'].includes(activeRoot)}>
          {#if visited.security}
            <SecuritySettings page={active} {settings} {form} {onSettingsUpdated} />
          {/if}
        </div>
        <div class:hidden={activeRoot !== 'gateway' && active !== 'runtime-ollama'}>
          {#if visited.gateway}
            <GatewaySettings page={active} />
          {/if}
        </div>
        <div class:hidden={activeRoot !== 'tools'}>
          {#if visited.tools}
            <ToolsSettings page={active} />
          {/if}
        </div>
        <div class:hidden={activeRoot !== 'developer'}>
          {#if visited.developer}
            <DeveloperApiSettings page={active} />
          {/if}
        </div>
        <div class:hidden={activeRoot !== 'otel'}>
          {#if visited.otel}
            <OtelSettings />
          {/if}
        </div>
        <div class:hidden={activeRoot !== 'automations'}>
          {#if visited.automations}
            <AutomationsSettings />
          {/if}
        </div>
        <div class:hidden={activeRoot !== 'appearance'}>
          {#if visited.appearance}
            <AppearanceSettings {settings} {form} {onSettingsUpdated} />
          {/if}
        </div>
        <div class:hidden={activeRoot !== 'updates'}>
          {#if visited.updates}
            <UpdatesSettings />
          {/if}
        </div>
        <div class:hidden={activeRoot !== 'privacy' && active !== 'diagnostics-reports'}>
          {#if visited.privacy}
            <PrivacySettings page={active === 'diagnostics-reports' ? 'reports' : 'privacy'} {form} />
          {/if}
        </div>
        <div class:hidden={active !== 'diagnostics-health'}>
          {#if visited.diagnostics}
            <DiagnosticsSettings />
          {/if}
        </div>
      </div>
    </div>

    {#if dirtyCount > 0 || saveError}
      <footer class="max-h-[45%] shrink-0 overflow-y-auto border-t border-border bg-card px-6 py-2.5">
        <div class="flex w-full max-w-5xl flex-wrap items-center justify-between gap-x-4 gap-y-2">
          <p
            class={cn('flex min-w-0 items-center gap-1.5 text-xs', saveError ? 'text-destructive' : 'text-muted-foreground')}
            role={saveError ? 'alert' : 'status'}
          >
            {#if saveError}
              <TriangleAlert class="size-3.5 shrink-0" /> <span class="break-words">{saveError}</span>
            {:else}
              <span class="size-1.5 shrink-0 rounded-full bg-primary" aria-hidden="true"></span>
              <span>{t('settings.unsaved_changes', { count: dirtyCount })}</span>
            {/if}
          </p>
          <div class="flex shrink-0 items-center gap-2">
            <Button variant="ghost" size="sm" onclick={discardChanges} disabled={saving}>{t('settings.discard')}</Button>
            <Button size="sm" onclick={saveSettings} disabled={saving || uiLevel.busy || dirtyCount === 0}>
              {#if saving}{t('settings.saving')}{:else}<Check /> {t('settings.save_btn')}{/if}
            </Button>
          </div>
        </div>
      </footer>
    {/if}
  </main>
</div>

<style>
  :global([data-setting-highlight]) {
    outline: 2px solid var(--ring);
    outline-offset: -2px;
  }
</style>
