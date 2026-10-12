<script lang="ts">
  // Settings > Runtime > Lemonade: the optional engine for Ryzen AI NPUs (FastFlowLM) and
  // its own ROCm / Vulkan llama.cpp builds. Install (SHA-256 checked), update / rollback by
  // build, start / stop, the NPU detection, the backends and Lemonade's model registry with
  // download and delete. Downloaded models are served by the gateway under their Lemonade names.
  import { invoke } from '$lib/ipc';
  import { onDestroy, onMount } from 'svelte';
  import { toast } from 'svelte-sonner';
  import type {
    LemonadeBackend,
    LemonadeModel,
    LemonadeModelList,
    LemonadeStatus,
    RuntimeEntry,
    RuntimeReleasePick,
  } from '$lib/types';
  import { t, type TKey } from '$lib/i18n.svelte';
  import { formatBytes } from '$lib/format';
  import { localizeError } from '$lib/network';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Badge } from '$lib/components/ui/badge';
  import { Progress } from '$lib/components/ui/progress';
  import { Cpu, ShieldCheck } from '@lucide/svelte';
  import SettingGroup from './SettingGroup.svelte';

  const fmt = (b: number) => formatBytes(b, 1);
  const uid = $props.id();

  let status = $state<LemonadeStatus | null>(null);
  let list = $state<LemonadeModelList | null>(null);
  /** The action in flight (`install`, `start`, `stop`, `refresh`, `pull:<id>`, `backend:<r>:<b>`...). */
  let busy = $state<string | null>(null);
  let latest = $state<RuntimeReleasePick | null>(null);
  let filter = $state('');
  let onlyDownloaded = $state(false);
  let timer: ReturnType<typeof setInterval> | null = null;

  const active = $derived(status?.installs.find((i) => i.active) ?? null);
  const npuText = $derived.by(() => {
    const n = status?.npu;
    if (!n || !n.detected) return t('lem.npu_none');
    return n.driver_installed ? t('lem.npu_detected', { name: n.name ?? 'AMD XDNA' }) : t('lem.npu_no_driver');
  });
  const update = $derived(
    latest && active && latest.tag !== active.tag && !status?.installs.some((i) => i.tag === latest?.tag) ? latest : null,
  );
  const progress = $derived.by(() => {
    const s = status;
    if (!s) return null;
    if (s.pull) {
      const total = s.pull.total_download_size > 0 ? s.pull.total_download_size : s.pull.bytes_total;
      return { label: `${s.pull.target} ${s.pull.file}`, done: s.pull.bytes_downloaded, total };
    }
    if (s.install_progress) {
      return { label: s.install_progress.current_asset, done: s.install_progress.downloaded_bytes, total: s.install_progress.total_bytes };
    }
    return null;
  });
  const shown = $derived.by(() => {
    const f = filter.trim().toLowerCase();
    return (list?.models ?? [])
      .filter((m) => (!onlyDownloaded || m.downloaded) && (!f || m.id.toLowerCase().includes(f) || m.recipe.toLowerCase().includes(f)))
      .sort((a, b) => Number(b.downloaded) - Number(a.downloaded) || Number(b.suggested) - Number(a.suggested) || a.id.localeCompare(b.id));
  });

  async function loadStatus() {
    try {
      status = await invoke<LemonadeStatus>('get_lemonade_status');
    } catch (e) {
      toast.error(localizeError(e));
    }
  }

  async function loadList() {
    try {
      list = await invoke<LemonadeModelList>('list_lemonade_models');
    } catch {
      /* a list is optional */
    }
  }

  function poll(on: boolean) {
    if (on && !timer) timer = setInterval(loadStatus, 700);
    if (!on && timer) {
      clearInterval(timer);
      timer = null;
    }
  }

  onMount(async () => {
    await loadStatus();
    await loadList();
  });
  onDestroy(() => poll(false));

  /** Runs one action with the status polled for its progress; reports failures as a toast. */
  async function run<T>(name: string, action: () => Promise<T>): Promise<T | undefined> {
    if (busy) return undefined;
    busy = name;
    poll(true);
    try {
      return await action();
    } catch (e) {
      toast.error(localizeError(e));
      return undefined;
    } finally {
      poll(false);
      busy = null;
      await loadStatus();
    }
  }

  async function install() {
    const entry = await run('install', () => invoke<RuntimeEntry>('install_lemonade', { tag: null }));
    if (entry) {
      toast.success(t('lem.installed_ok', { name: entry.name }));
      latest = null;
    }
  }

  async function checkUpdate() {
    await run('check', async () => {
      latest = await invoke<RuntimeReleasePick>('preview_lemonade_release', { tag: null });
    });
  }

  async function installUpdate() {
    const entry = await run('install', () => invoke<RuntimeEntry>('install_lemonade', { tag: update?.tag ?? null }));
    if (entry) {
      toast.success(t('lem.installed_ok', { name: entry.name }));
      latest = null;
    }
  }

  async function removeBuild(id: string) {
    await run(`remove:${id}`, async () => {
      await invoke('remove_lemonade_install', { id });
      toast.success(t('lem.removed'));
    });
  }

  async function saveSettings(patch: { backend?: string; runtime?: string | null }) {
    if (!status) return;
    await run('settings', async () => {
      await invoke('save_lemonade_settings', { settings: { ...status!.settings, ...patch } });
    });
  }

  async function start() {
    await run('start', async () => {
      await invoke('start_lemonade');
      await loadList();
    });
  }

  async function stop() {
    await run('stop', () => invoke('stop_lemonade'));
  }

  async function refresh() {
    await run('refresh', async () => {
      list = await invoke<LemonadeModelList>('refresh_lemonade_models');
    });
  }

  async function pull(m: LemonadeModel) {
    await run(`pull:${m.id}`, async () => {
      await invoke('pull_lemonade_model', { model: m.id });
      toast.success(t('lem.pulled', { model: m.id }));
      await loadList();
    });
  }

  async function remove(m: LemonadeModel) {
    await run(`delete:${m.id}`, async () => {
      await invoke('delete_lemonade_model', { model: m.id });
      toast.success(t('lem.deleted', { model: m.id }));
      await loadList();
    });
  }

  async function installBackend(b: LemonadeBackend) {
    await run(`backend:${b.recipe}:${b.backend}`, async () => {
      await invoke('install_lemonade_backend', { recipe: b.recipe, backend: b.backend });
      toast.success(t('lem.backend_ok', { name: `${b.recipe}:${b.backend}` }));
    });
  }

  const BACKEND_CHOICES = ['auto', 'vulkan', 'rocm', 'cpu'];
  const stateKey = (s: string) => (['installed', 'installable', 'update_required'].includes(s) ? s : 'unsupported');
  const needsNpu = (b: LemonadeBackend) => b.backend === 'npu' && !status?.npu.detected;
  /** Backends worth showing: the usable ones, plus the NPU row so the reason is visible. */
  const backends = $derived((status?.backends ?? []).filter((b) => b.state !== 'unsupported' || b.backend === 'npu'));
</script>

<SettingGroup title={t('lem.title')} hint={t('lem.hint')}>
  <div class="flex flex-col divide-y divide-border">
    {#if status && !status.supported}
      <p class="px-4 py-3 text-sm text-muted-foreground">{t('lem.unsupported')}</p>
    {:else if status}
      <!-- Install state and builds -->
      <div class="flex flex-col gap-2 px-4 py-3">
        <div class="flex min-w-0 items-center justify-between gap-3">
          <div class="flex min-w-0 flex-col gap-0.5">
            <span class="flex min-w-0 items-center gap-1.5 text-sm font-medium">
              {active ? t('lem.installed') : t('lem.not_installed')}
              {#if active?.version}<Badge variant="outline" class="font-mono text-xs">{active.version}</Badge>{/if}
              <ShieldCheck class="size-3.5 text-success" aria-hidden="true" />
            </span>
            {#if latest && !update && active}
              <span class="text-xs text-muted-foreground" role="status">{t('lem.up_to_date', { tag: latest.tag })}</span>
            {:else if update}
              <span class="text-xs text-warning" role="status">{t('lem.update_available', { tag: update.tag })}</span>
            {/if}
          </div>
          <div class="flex shrink-0 items-center gap-1.5">
            {#if !active}
              <Button size="sm" disabled={busy !== null} onclick={install}>
                {busy === 'install' ? t('lem.installing') : t('lem.install')}
              </Button>
            {:else}
              {#if update}
                <Button size="sm" disabled={busy !== null} onclick={installUpdate}>
                  {busy === 'install' ? t('lem.installing') : t('lem.update_install')}
                </Button>
              {:else}
                <Button variant="outline" size="sm" disabled={busy !== null} onclick={checkUpdate}>
                  {busy === 'check' ? t('lem.checking') : t('lem.check_update')}
                </Button>
              {/if}
            {/if}
          </div>
        </div>
        {#if busy === 'install' && progress}
          <Progress value={progress.total > 0 ? (progress.done / progress.total) * 100 : 0} aria-label={progress.label} />
          <span class="font-num text-xs text-muted-foreground">{fmt(progress.done)}{progress.total > 0 ? ` / ${fmt(progress.total)}` : ''}</span>
        {/if}
        {#if status.installs.length > 1}
          <div class="flex flex-col gap-1" role="group" aria-label={t('lem.builds')}>
            <span class="text-xs font-medium text-muted-foreground">{t('lem.builds')}</span>
            {#each status.installs as b (b.id)}
              <div class="flex min-w-0 items-center justify-between gap-2 text-xs">
                <span class="flex min-w-0 items-center gap-1.5">
                  <span class="truncate">{b.name}</span>
                  {#if b.version}<Badge variant="outline" class="font-mono">{b.version}</Badge>{/if}
                  {#if b.active}<Badge variant="secondary">{t('lem.build_active')}</Badge>{/if}
                </span>
                <span class="flex shrink-0 gap-1">
                  {#if !b.active}
                    <Button variant="outline" size="sm" disabled={busy !== null} onclick={() => saveSettings({ runtime: b.id })}>{t('lem.use_build')}</Button>
                  {:else if status.settings.runtime}
                    <Button variant="outline" size="sm" disabled={busy !== null} onclick={() => saveSettings({ runtime: null })}>{t('lem.use_newest')}</Button>
                  {/if}
                  <Button variant="ghost" size="sm" class="text-destructive" disabled={busy !== null || status.running_runtime === b.id} onclick={() => removeBuild(b.id)}>{t('lem.remove')}</Button>
                </span>
              </div>
            {/each}
          </div>
        {/if}
      </div>

      <!-- NPU -->
      <div class="flex min-w-0 flex-col gap-0.5 px-4 py-3">
        <span class="flex items-center gap-1.5 text-sm font-medium">
          <Cpu class="size-3.5" aria-hidden="true" />
          {t('lem.npu')}
          <Badge variant={status.npu.detected && status.npu.driver_installed ? 'secondary' : 'outline'} class="text-xs" data-testid="lemonade-npu">{npuText}</Badge>
        </span>
        {#if !status.npu.detected}
          <span class="text-xs text-muted-foreground">{t('lem.npu_none_hint')}</span>
        {/if}
      </div>

      {#if active}
        <!-- Server -->
        <div class="flex min-w-0 flex-col gap-2 px-4 py-3">
          <div class="flex min-w-0 items-center justify-between gap-3">
            <div class="flex min-w-0 flex-col gap-0.5">
              <span class="text-sm font-medium">{t('lem.engine')}</span>
              <span class="truncate font-mono text-xs text-muted-foreground" data-testid="lemonade-state">
                {status.running ? t('lem.running', { url: status.base_url ?? '' }) : t('lem.stopped')}
              </span>
            </div>
            {#if status.running}
              <Button variant="outline" size="sm" disabled={busy !== null} onclick={stop}>{t('lem.stop')}</Button>
            {:else}
              <Button size="sm" disabled={busy !== null} onclick={start}>{busy === 'start' ? t('lem.starting') : t('lem.start')}</Button>
            {/if}
          </div>
          <label class="flex items-center justify-between gap-3 text-xs" for="{uid}-backend">
            <span class="flex flex-col">
              <span class="text-sm font-medium">{t('lem.backend')}</span>
              <span class="text-muted-foreground">{t('lem.backend_help')}</span>
            </span>
            <select
              id="{uid}-backend"
              class="h-8 shrink-0 rounded-md border border-input bg-background px-2 text-xs"
              value={status.settings.backend}
              disabled={busy !== null}
              onchange={(e) => saveSettings({ backend: e.currentTarget.value })}
            >
              {#each BACKEND_CHOICES as b (b)}
                <option value={b}>{t(`lem.backend_${b}` as TKey)}</option>
              {/each}
            </select>
          </label>
          <span class="text-xs text-muted-foreground">{t('lem.restart_note')}</span>
        </div>

        <!-- Backends -->
        {#if backends.length > 0}
          <div class="flex flex-col gap-1.5 px-4 py-3">
            <span class="text-sm font-medium">{t('lem.backends')}</span>
            {#each backends as b (`${b.recipe}:${b.backend}`)}
              <div class="flex min-w-0 items-center justify-between gap-2 text-xs">
                <span class="flex min-w-0 items-center gap-1.5">
                  <span class="font-mono">{b.recipe}:{b.backend}</span>
                  {#if b.version}<Badge variant="outline" class="font-mono">{b.version}</Badge>{/if}
                  <span class="text-muted-foreground">{t(`lem.backend_state_${stateKey(b.state)}` as TKey)}</span>
                </span>
                {#if b.state === 'installable' || b.state === 'update_required'}
                  <Button variant="outline" size="sm" disabled={busy !== null || needsNpu(b)} onclick={() => installBackend(b)}>
                    {busy === `backend:${b.recipe}:${b.backend}` ? t('lem.pulling') : needsNpu(b) ? t('lem.backend_needs_npu') : t('lem.backend_install')}
                  </Button>
                {:else if needsNpu(b)}
                  <span class="text-muted-foreground">{t('lem.backend_needs_npu')}</span>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        <!-- Models -->
        <div class="flex flex-col gap-2 px-4 py-3">
          <div class="flex min-w-0 items-center justify-between gap-3">
            <div class="flex min-w-0 flex-col">
              <span class="text-sm font-medium">{t('lem.models')}</span>
              {#if list && list.updated_at > 0}
                <span class="text-xs text-muted-foreground">{list.live ? t('lem.live') : t('lem.snapshot')}</span>
              {/if}
            </div>
            <Button variant="outline" size="sm" disabled={busy !== null} onclick={refresh}>
              {busy === 'refresh' ? t('lem.refreshing') : t('lem.refresh')}
            </Button>
          </div>
          <p class="text-xs text-muted-foreground">{t('lem.models_hint')}</p>
          {#if busy?.startsWith('pull:') && progress}
            <Progress value={progress.total > 0 ? (progress.done / progress.total) * 100 : 0} aria-label={progress.label} />
            <span class="font-num text-xs text-muted-foreground">{fmt(progress.done)}{progress.total > 0 ? ` / ${fmt(progress.total)}` : ''}</span>
          {/if}
          {#if list && list.models.length > 0}
            <div class="flex items-center gap-3">
              <Input class="h-8 text-xs" placeholder={t('lem.filter')} aria-label={t('lem.filter')} bind:value={filter} />
              <label class="flex shrink-0 items-center gap-1.5 text-xs">
                <input type="checkbox" bind:checked={onlyDownloaded} />
                {t('lem.only_downloaded')}
              </label>
            </div>
            <div class="flex max-h-72 flex-col divide-y divide-border overflow-y-auto rounded-md border border-border" data-testid="lemonade-models">
              {#each shown as m (m.id)}
                <div class="flex min-w-0 items-center justify-between gap-2 px-3 py-1.5 text-xs">
                  <span class="flex min-w-0 flex-col">
                    <span class="truncate font-medium" title={m.checkpoint}>{m.id}</span>
                    <span class="flex items-center gap-1.5 text-muted-foreground">
                      {m.recipe} · {t(`lem.kind_${m.kind}` as TKey)} · {m.size_gb.toFixed(2)} GB
                      {#if m.kind !== 'chat' && m.kind !== 'embedding' && m.kind !== 'rerank' && m.kind !== 'transcription'}
                        · {t('lem.not_served')}
                      {/if}
                    </span>
                  </span>
                  <span class="flex shrink-0 items-center gap-1.5">
                    {#if m.downloaded}
                      <Badge variant="secondary">{t('lem.downloaded')}</Badge>
                      <Button variant="ghost" size="sm" class="text-destructive" disabled={busy !== null} onclick={() => remove(m)}>{t('lem.delete')}</Button>
                    {:else}
                      <Button variant="outline" size="sm" disabled={busy !== null} onclick={() => pull(m)}>
                        {busy === `pull:${m.id}` ? t('lem.pulling') : t('lem.pull')}
                      </Button>
                    {/if}
                  </span>
                </div>
              {/each}
            </div>
          {:else}
            <p class="text-xs text-muted-foreground">{t('lem.no_models')}</p>
          {/if}
        </div>
      {/if}

      <div class="flex flex-col gap-1 px-4 py-3">
        <p class="text-xs text-muted-foreground">{t('lem.license')}</p>
        <p class="truncate font-mono text-xs text-muted-foreground" title={status.models_dir}>{t('lem.paths', { dir: status.models_dir })}</p>
      </div>
    {/if}
  </div>
</SettingGroup>
