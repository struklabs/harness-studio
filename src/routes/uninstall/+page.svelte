<script lang="ts">
  // Uninstall window. Started by `harness-studio --uninstall` (Settings > Storage, or the Windows
  // uninstaller with --installer). A small Rust app serves it (src-tauri/src/uninstall.rs), so
  // nothing of the normal app is running while files are deleted.
  import { onMount } from 'svelte';
  import { fade, fly } from 'svelte/transition';
  import { listen } from '@tauri-apps/api/event';
  import CheckCircle from '@lucide/svelte/icons/circle-check';
  import Circle from '@lucide/svelte/icons/circle';
  import Eraser from '@lucide/svelte/icons/eraser';
  import Loader from '@lucide/svelte/icons/loader-circle';
  import ShieldCheck from '@lucide/svelte/icons/shield-check';
  import Sliders from '@lucide/svelte/icons/sliders-horizontal';
  import Warning from '@lucide/svelte/icons/triangle-alert';
  import { invoke } from '#lib/ipc.js';
  import { setLanguage, t, type Language, type TKey } from '#lib/i18n.svelte.js';
  import { applyThemeSettings } from '#lib/theme.js';
  import { hideSplash } from '#lib/splash.js';
  import { formatBytes } from '#lib/format.js';
  import { localizeError } from '#lib/network.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Progress } from '#lib/components/ui/progress/index.js';
  import {
    CATEGORY_ORDER,
    applyStepEvent,
    buildSelection,
    externalAllowed,
    failedSteps,
    initialSelection,
    initialSteps,
    keptCategories,
    progressOf,
    removedCategories,
    stepIds,
    totalBytes,
    type CategoryId,
    type CategoryInfo,
    type Estimate,
    type Mode,
    type Step,
    type StepEvent,
  } from '#lib/uninstall.js';

  interface Context {
    installer: boolean;
    kind: 'windows_installed' | 'mac_app' | 'linux_deb' | 'linux_app_image' | 'portable';
    version: string;
    data_dir: string;
    language: string;
    theme: string;
    color_theme: string;
    cvd_safe: boolean;
  }
  type Screen = 'choose' | 'review' | 'progress' | 'done';

  let ctx = $state<Context | null>(null);
  let screen = $state<Screen>('choose');
  let mode = $state<Mode>('choose');
  let infos = $state<CategoryInfo[]>([]);
  let externalFolders = $state<string[]>([]);
  let selected = $state(new Set<CategoryId>());
  let externalSelected = $state<string[]>([]);
  let steps = $state<Step[]>([]);
  let loadError = $state<string | null>(null);
  let runError = $state<string | null>(null);
  let finishError = $state<string | null>(null);
  let busy = $state(false);
  let appRemoved = $state(false);
  let ready = $state(false);

  const reduced = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  const dur = (ms: number) => (reduced ? 0 : ms);

  const removed = $derived(removedCategories(mode, infos, selected));
  const kept = $derived(keptCategories(mode, infos, selected));
  const freed = $derived(totalBytes(infos, removed));
  const showExternal = $derived(externalFolders.length > 0 && externalAllowed(mode, selected));
  const failed = $derived(failedSteps(steps));
  const screens: { id: Screen; label: TKey }[] = [
    { id: 'choose', label: 'un.step_choose' },
    { id: 'review', label: 'un.step_review' },
    { id: 'progress', label: 'un.step_remove' },
    { id: 'done', label: 'un.step_done' },
  ];
  const screenIndex = $derived(screens.findIndex((s) => s.id === screen));

  const infoOf = (id: CategoryId) => infos.find((i) => i.category === id);
  const catName = (id: CategoryId) => t(`un.cat_${id}` as TKey);
  const catDesc = (id: CategoryId) => t(`un.cat_${id}_desc` as TKey);
  function stepLabel(id: string): string {
    if (id === 'stop') return t('un.step_stop_label');
    if (id === 'credentials') return t('un.step_credentials_label');
    if (id === 'leftovers') return t('un.step_leftovers_label');
    return catName(id as CategoryId);
  }
  function sizeText(bytes: number | null): string {
    return bytes === null ? t('un.size_calculating') : formatBytes(bytes, 1);
  }

  /** Sizes arrive over several calls; keep what the user already ticked. */
  function mergeEstimate(est: Estimate, first: boolean) {
    infos = est.categories;
    externalFolders = est.external_models;
    if (first) selected = initialSelection(est.categories);
  }

  async function loadEstimate() {
    let first = true;
    for (let attempt = 0; attempt < 120; attempt++) {
      try {
        const est = await invoke<Estimate>('uninstall_estimate');
        mergeEstimate(est, first);
        first = false;
        if (est.complete) return;
      } catch (e) {
        loadError = localizeError(e);
        return;
      }
      await new Promise((r) => setTimeout(r, 400));
    }
  }

  onMount(() => {
    let off: (() => void) | undefined;
    void (async () => {
      try {
        ctx = await invoke<Context>('uninstall_context');
        setLanguage((ctx.language === 'en' ? 'en' : 'tr') as Language);
        applyThemeSettings({ theme: ctx.theme, color_theme: ctx.color_theme, cvd_safe: ctx.cvd_safe });
        document.title = t('un.window_title');
      } catch (e) {
        loadError = localizeError(e);
      }
      ready = true;
      hideSplash();
      off = await listen<StepEvent>('uninstall-step', (event) => {
        steps = applyStepEvent(steps, event.payload);
      });
      await loadEstimate();
    })();
    return () => off?.();
  });

  function toggle(id: CategoryId, on: boolean) {
    const next = new Set(selected);
    if (on) next.add(id);
    else next.delete(id);
    selected = next;
  }
  function toggleExternal(path: string, on: boolean) {
    externalSelected = on ? [...externalSelected, path] : externalSelected.filter((p) => p !== path);
  }
  const selectRecommended = () => (selected = initialSelection(infos));
  const selectNone = () => (selected = new Set());

  function cancel() {
    void invoke('uninstall_exit', { code: 2 });
  }

  async function start() {
    busy = true;
    runError = null;
    steps = initialSteps(stepIds(mode, removed));
    screen = 'progress';
    try {
      await invoke('uninstall_run', { selection: buildSelection(mode, selected, externalSelected) });
    } catch (e) {
      runError = localizeError(e);
    }
    busy = false;
    screen = 'done';
  }

  async function finishApp() {
    busy = true;
    finishError = null;
    try {
      await invoke('uninstall_finish');
      appRemoved = true;
      setTimeout(() => void invoke('uninstall_exit', { code: 0 }), 1200);
    } catch (e) {
      finishError = localizeError(e);
      busy = false;
    }
  }
  const closeDone = () => void invoke('uninstall_exit', { code: 0 });

  const appAction = $derived.by(() => {
    switch (ctx?.kind) {
      case 'windows_installed': return { label: t('un.app_remove_windows'), hint: '' };
      case 'mac_app': return { label: t('un.app_remove_mac'), hint: '' };
      case 'linux_deb': return { label: t('un.app_remove_deb'), hint: t('un.app_remove_deb_hint') };
      case 'linux_app_image': return { label: t('un.app_remove_appimage'), hint: t('un.app_remove_appimage_hint') };
      default: return null;
    }
  });
</script>

<div class="flex h-screen min-h-0 flex-col bg-background text-foreground">
  <header class="flex flex-col gap-3 border-b border-border px-6 pt-5 pb-4">
    <h1 class="text-lg font-semibold tracking-tight">{t('un.window_title')}</h1>
    <ol class="flex items-center gap-2 text-xs" aria-label={t('un.window_title')}>
      {#each screens as s, i (s.id)}
        <li class="flex items-center gap-2" aria-current={i === screenIndex ? 'step' : undefined}>
          <span
            class="flex size-5 items-center justify-center rounded-full text-[0.65rem] font-semibold transition-colors duration-300 {i <= screenIndex ? 'bg-primary text-primary-foreground' : 'bg-muted text-muted-foreground'}"
          >{i + 1}</span>
          <span class={i === screenIndex ? 'font-medium text-foreground' : 'text-muted-foreground'}>{t(s.label)}</span>
          {#if i < screens.length - 1}<span class="mx-1 h-px w-6 bg-border" aria-hidden="true"></span>{/if}
        </li>
      {/each}
    </ol>
  </header>

  <main class="min-h-0 flex-1 overflow-y-auto px-6 py-5">
    {#if !ready}
      <div class="flex h-full items-center justify-center text-muted-foreground"><Loader class="size-5 animate-spin" /></div>
    {:else if loadError}
      <div class="flex flex-col gap-2" role="alert">
        <h2 class="text-base font-semibold">{t('un.load_error')}</h2>
        <p class="text-sm text-destructive">{loadError}</p>
      </div>
    {:else if screen === 'choose'}
      <section class="flex flex-col gap-4" in:fly={{ x: 16, duration: dur(200) }}>
        <div>
          <h2 class="text-base font-semibold">{t('un.choose_title')}</h2>
          <p class="mt-1 text-sm text-muted-foreground">{t('un.choose_desc')}</p>
        </div>

        <div class="grid gap-3 sm:grid-cols-2" role="radiogroup" aria-label={t('un.choose_title')}>
          {#each [
            { id: 'everything', icon: Eraser, title: 'un.opt_everything', desc: 'un.opt_everything_desc' },
            { id: 'choose', icon: Sliders, title: 'un.opt_choose', desc: 'un.opt_choose_desc' },
          ] as option (option.id)}
            {@const Icon = option.icon}
            <label
              class="group flex cursor-pointer flex-col gap-2 rounded-xl border p-4 transition-all duration-200 has-[:focus-visible]:ring-3 has-[:focus-visible]:ring-ring/50 {mode === option.id ? (option.id === 'everything' ? 'border-destructive bg-destructive/5' : 'border-primary bg-primary/5') : 'border-border bg-card hover:border-foreground/30'}"
            >
              <input type="radio" name="mode" class="sr-only" value={option.id} checked={mode === option.id} onchange={() => (mode = option.id as Mode)} />
              <span class="flex items-center gap-2 text-sm font-semibold {mode === option.id && option.id === 'everything' ? 'text-destructive' : ''}">
                <Icon class="size-4" />{t(option.title as TKey)}
              </span>
              <span class="text-xs leading-relaxed text-muted-foreground">{t(option.desc as TKey)}</span>
            </label>
          {/each}
        </div>

        {#if mode === 'everything'}
          <p class="flex items-start gap-2 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive" role="alert" in:fade={{ duration: dur(150) }}>
            <Warning class="mt-0.5 size-4 shrink-0" />{t('un.everything_warn')}
          </p>
        {/if}

        {#if mode === 'choose'}
          <div class="flex flex-col gap-2" in:fly={{ y: 8, duration: dur(200) }}>
            <div class="flex items-center justify-end gap-1">
              <Button variant="ghost" size="xs" onclick={selectRecommended}>{t('un.select_defaults')}</Button>
              <Button variant="ghost" size="xs" onclick={selectNone}>{t('un.select_none')}</Button>
            </div>
            <ul class="flex flex-col gap-1.5">
              {#each CATEGORY_ORDER as id (id)}
                {@const info = infoOf(id)}
                {@const absent = info !== undefined && !info.present}
                <li>
                  <label
                    class="flex items-start gap-3 rounded-lg border p-3 transition-colors duration-150 has-[:focus-visible]:ring-3 has-[:focus-visible]:ring-ring/50 {absent ? 'cursor-default border-transparent opacity-50' : selected.has(id) ? 'cursor-pointer border-primary/40 bg-primary/5' : 'cursor-pointer border-border bg-card hover:border-foreground/30'}"
                  >
                    <input
                      type="checkbox"
                      class="mt-0.5 size-4 accent-[var(--primary)]"
                      checked={selected.has(id)}
                      disabled={absent}
                      onchange={(e) => toggle(id, e.currentTarget.checked)}
                    />
                    <span class="flex min-w-0 flex-1 flex-col gap-0.5">
                      <span class="text-sm font-medium">{catName(id)}</span>
                      <span class="text-xs text-muted-foreground">{catDesc(id)}</span>
                    </span>
                    <span class="shrink-0 text-xs tabular-nums text-muted-foreground">
                      {#if absent}{t('un.not_found')}{:else}{sizeText(info?.bytes ?? null)}{/if}
                    </span>
                  </label>
                </li>
              {/each}
            </ul>
          </div>
        {/if}

        {#if showExternal}
          <div class="flex flex-col gap-2 rounded-lg border border-border p-3" in:fade={{ duration: dur(150) }}>
            <p class="text-sm font-medium">{t('un.external_title')}</p>
            <p class="text-xs text-muted-foreground">{t('un.external_desc')}</p>
            {#each externalFolders as folder (folder)}
              <label class="flex items-center gap-2 break-all font-mono text-xs">
                <input type="checkbox" class="size-4 accent-[var(--primary)]" checked={externalSelected.includes(folder)} onchange={(e) => toggleExternal(folder, e.currentTarget.checked)} />
                {folder}
              </label>
            {/each}
          </div>
        {/if}
      </section>
    {:else if screen === 'review'}
      <section class="flex flex-col gap-4" in:fly={{ x: 16, duration: dur(200) }}>
        <div>
          <h2 class="text-base font-semibold">{t('un.review_title')}</h2>
          <p class="mt-1 text-sm text-muted-foreground">{t('un.review_desc')}</p>
        </div>

        <div class="rounded-xl border border-destructive/30 bg-destructive/5 p-4">
          <h3 class="mb-2 flex items-center gap-2 text-sm font-semibold text-destructive"><Eraser class="size-4" />{t('un.will_delete')}</h3>
          <ul class="flex flex-col gap-1 text-sm">
            <li class="flex justify-between gap-3"><span>{t('un.app_files')}</span></li>
            {#each removed as id (id)}
              <li class="flex justify-between gap-3"><span>{catName(id)}</span><span class="text-xs tabular-nums text-muted-foreground">{sizeText(infoOf(id)?.bytes ?? null)}</span></li>
            {/each}
            {#each showExternal ? externalSelected : [] as folder (folder)}
              <li class="break-all font-mono text-xs">{folder}</li>
            {/each}
          </ul>
          {#if removed.length === 0}<p class="mt-2 text-xs text-muted-foreground">{t('un.nothing_deleted')}</p>{/if}
          {#if removed.length > 0}
            <p class="mt-3 border-t border-destructive/20 pt-2 text-xs font-medium">
              {freed.pending ? t('un.freed_more', { size: formatBytes(freed.bytes, 1) }) : t('un.freed', { size: formatBytes(freed.bytes, 1) })}
            </p>
          {/if}
        </div>

        <div class="rounded-xl border border-border bg-card p-4">
          <h3 class="mb-2 flex items-center gap-2 text-sm font-semibold"><ShieldCheck class="size-4 text-primary" />{t('un.will_keep')}</h3>
          {#if kept.length === 0}
            <p class="text-xs text-muted-foreground">{t('un.nothing_kept')}</p>
          {:else}
            <ul class="flex flex-col gap-1 text-sm">
              {#each kept as id (id)}
                <li class="flex justify-between gap-3"><span>{catName(id)}</span><span class="text-xs tabular-nums text-muted-foreground">{sizeText(infoOf(id)?.bytes ?? null)}</span></li>
              {/each}
            </ul>
          {/if}
        </div>
      </section>
    {:else if screen === 'progress'}
      <section class="flex flex-col gap-4" in:fly={{ x: 16, duration: dur(200) }}>
        <div>
          <h2 class="text-base font-semibold">{t('un.progress_title')}</h2>
          <p class="mt-1 text-sm text-muted-foreground">{t('un.progress_desc')}</p>
        </div>
        <Progress value={Math.round(progressOf(steps) * 100)} max={100} class="h-2" />
        <ul class="flex flex-col gap-2" aria-live="polite">
          {#each steps as step (step.id)}
            <li class="flex items-center gap-3 text-sm">
              {#if step.state === 'done'}<CheckCircle class="size-4 text-primary" />
              {:else if step.state === 'running'}<Loader class="size-4 animate-spin text-primary" />
              {:else if step.state === 'failed'}<Warning class="size-4 text-destructive" />
              {:else}<Circle class="size-4 text-muted-foreground/50" />{/if}
              <span class={step.state === 'pending' ? 'text-muted-foreground' : ''}>{stepLabel(step.id)}</span>
            </li>
          {/each}
        </ul>
      </section>
    {:else}
      <section class="flex flex-col gap-4" in:fly={{ x: 16, duration: dur(200) }}>
        <div class="flex items-start gap-3">
          {#if failed.length || runError}
            <Warning class="mt-0.5 size-6 text-destructive" />
          {:else}
            <CheckCircle class="mt-0.5 size-6 text-primary" />
          {/if}
          <div>
            <h2 class="text-base font-semibold">{failed.length || runError ? t('un.done_partial_title') : t('un.done_title')}</h2>
            <p class="mt-1 text-sm text-muted-foreground">
              {#if failed.length || runError}{t('un.done_partial_desc')}
              {:else if ctx?.installer}{t('un.done_desc_installer')}
              {:else}{t('un.done_desc_manual')}{/if}
            </p>
          </div>
        </div>
        {#if failed.length}
          <ul class="flex flex-col gap-2" role="alert">
            {#each failed as step (step.id)}
              <li class="rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm">
                <p class="font-medium">{stepLabel(step.id)}</p>
                <p class="mt-0.5 break-all text-xs text-muted-foreground">{step.error}</p>
              </li>
            {/each}
          </ul>
        {:else if runError}
          <p class="break-all text-xs text-destructive" role="alert">{runError}</p>
        {/if}
        {#if !ctx?.installer}
          {#if appRemoved}
            <p class="text-sm text-primary" role="status">{t('un.app_removed')}</p>
          {:else if appAction}
            <div class="flex flex-col gap-2 rounded-xl border border-border bg-card p-4">
              <Button variant="destructive" onclick={finishApp} disabled={busy}>{appAction.label}</Button>
              {#if appAction.hint}<p class="text-xs text-muted-foreground">{appAction.hint}</p>{/if}
              {#if finishError}<p class="text-xs text-destructive" role="alert">{finishError}</p>{/if}
            </div>
          {:else}
            <p class="text-sm text-muted-foreground">{t('un.app_remove_portable')}</p>
          {/if}
        {/if}
      </section>
    {/if}
  </main>

  <footer class="flex items-center justify-between gap-2 border-t border-border px-6 py-3">
    {#if screen === 'choose'}
      <Button variant="ghost" onclick={cancel}>{t('un.cancel')}</Button>
      <Button onclick={() => (screen = 'review')} disabled={!ready || !!loadError}>{t('un.review')}</Button>
    {:else if screen === 'review'}
      <Button variant="ghost" onclick={() => (screen = 'choose')} disabled={busy}>{t('un.back')}</Button>
      <Button variant="destructive" onclick={start} disabled={busy}>{t('un.remove_btn')}</Button>
    {:else if screen === 'done'}
      <span></span>
      <Button variant={ctx?.installer ? 'default' : 'outline'} onclick={closeDone}>{ctx?.installer ? t('un.finish') : t('un.close')}</Button>
    {:else}
      <span></span><span></span>
    {/if}
  </footer>
</div>
