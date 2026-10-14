<script lang="ts">
  import HelpPopover from '../common/HelpPopover.svelte';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  // Settings > Runtime > Additional runtimes: the registry of extra llama-server builds (forks
  // such as PrismML's for Bonsai, or local builds). Add from a GitHub release (archive checked
  // against GitHub's SHA-256 digest; an archive without one only after explicit consent) or a
  // local folder (hashed; used in place or copied), verify, remove.
  import { invoke } from '#lib/ipc.js';
  import { onDestroy, onMount } from 'svelte';
  import { toast } from 'svelte-sonner';
  import type {
    BackendStatus,
    RuntimeDownloadProgress,
    RuntimeEntry,
    RuntimeReleasePick,
    RuntimeUsers,
    RuntimeVerifyReport,
    RuntimeView,
  } from '#lib/types.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { pathExample } from '#lib/platform.js';
  import { formatBytes } from '#lib/format.js';
  import { localizeError } from '#lib/network.js';
  import { runtimeInstallErrorText } from '#lib/runtimeBackend.js';
  import { runtimesStore, runtimeBuildLabel } from '#lib/stores/runtimes.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Checkbox } from '#lib/components/ui/checkbox/index.js';
  import { Progress } from '#lib/components/ui/progress/index.js';
  import { cn } from '#lib/utils.js';
  import { Check, FolderOpen, ShieldCheck, TriangleAlert } from '@lucide/svelte';
  import SettingGroup from './SettingGroup.svelte';

  const uid = $props.id();
  const fmt = (b: number) => formatBytes(b, 1);
  const BACKENDS = ['vulkan', 'cuda', 'rocm', 'sycl', 'cpu', 'metal'];

  let backendStatus = $state<BackendStatus | null>(null);
  onMount(() => {
    void runtimesStore.load();
    void invoke<BackendStatus>('get_backend_status').then((s) => { backendStatus = s; }).catch(() => {});
  });

  let extra = $derived(runtimesStore.list.filter((r) => r.kind === 'github' || r.kind === 'local'));

  // --- per-runtime actions -------------------------------------------------
  let busyId = $state<string | null>(null);
  let reports = $state<Record<string, RuntimeVerifyReport>>({});
  let removing = $state<{ id: string; users: RuntimeUsers | null } | null>(null);

  async function verify(id: string, acceptChanges = false) {
    busyId = id;
    try {
      reports[id] = await invoke<RuntimeVerifyReport>('verify_runtime', { id, acceptChanges });
      await runtimesStore.load(true);
    } catch (e) {
      toast.error(localizeError(e));
    } finally {
      busyId = null;
    }
  }

  async function askRemove(id: string) {
    removing = { id, users: null };
    try {
      const users = await invoke<RuntimeUsers>('get_runtime_users', { id });
      if (removing?.id === id) removing = { id, users };
    } catch {
      /* the remove call reports it */
    }
  }

  async function remove(r: RuntimeView, force: boolean) {
    busyId = r.id;
    try {
      await invoke('remove_runtime', { id: r.id, force });
      toast.success(t('rt.removed', { name: r.name || r.id }));
      removing = null;
      delete reports[r.id];
      await runtimesStore.load(true);
    } catch (e) {
      toast.error(localizeError(e));
    } finally {
      busyId = null;
    }
  }

  function usersList(u: RuntimeUsers | null): string {
    if (!u) return '';
    return [...u.running, ...u.definitions, ...u.profiles].join(', ');
  }

  function kindLabel(r: RuntimeView): string {
    const src = r.entry?.source;
    if (src?.kind === 'github') return t('rt.kind_github');
    if (src?.kind === 'local') return src.copied ? t('rt.kind_local_copy') : t('rt.kind_local');
    return t('rt.kind_official');
  }

  function sourceText(e: RuntimeEntry | null): string {
    if (!e) return '';
    return e.source.kind === 'github' ? `${e.source.repo} · ${e.source.tag}` : e.source.path;
  }

  // --- add from GitHub -----------------------------------------------------
  let repo = $state('');
  /** Runtime family of the release (`llama.cpp`, `whisper.cpp` for speech recognition or `sherpa-onnx` for text to speech). */
  let family = $state('llama.cpp');
  let tag = $state('');
  let backend = $state('');
  let ghName = $state('');
  let pick = $state<RuntimeReleasePick | null>(null);
  let acceptUnverified = $state(false);
  let finding = $state(false);
  let installing = $state(false);
  let ghError = $state<string | null>(null);
  let progress = $state<RuntimeDownloadProgress | null>(null);
  let timer: ReturnType<typeof setInterval> | null = null;
  onDestroy(() => timer && clearInterval(timer));

  let pickSize = $derived(pick ? pick.assets.reduce((n, a) => n + a.size, 0) : 0);
  let pct = $derived(progress && progress.total_bytes > 0 ? (progress.downloaded_bytes / progress.total_bytes) * 100 : 0);

  function query() {
    return { family, repo: repo.trim(), tag: tag.trim() || null, backend: backend || null, asset: null };
  }

  async function findRelease() {
    finding = true;
    ghError = null;
    pick = null;
    acceptUnverified = false;
    try {
      pick = await invoke<RuntimeReleasePick>('preview_runtime_release', { query: query() });
    } catch (e) {
      ghError = localizeError(e);
    } finally {
      finding = false;
    }
  }

  async function install() {
    if (!pick) return;
    installing = true;
    ghError = null;
    timer = setInterval(async () => {
      try {
        progress = await invoke<RuntimeDownloadProgress | null>('get_runtime_install_progress');
      } catch {
        /* best effort */
      }
    }, 500);
    try {
      const entry = await invoke<RuntimeEntry>('install_runtime_github', {
        request: {
          family: pick.family,
          repo: pick.repo,
          tag: pick.tag,
          // The previewed tag and backend pick the same archives again, a CUDA build's
          // `cudart-*` runtime included; naming the first archive left that one out.
          asset: null,
          backend: pick.backend || backend || null,
          name: ghName.trim() || null,
          id: null,
          allow_unverified: pick.unverified.length > 0 && acceptUnverified,
        },
      });
      toast.success(t('rt.added', { name: entry.name }));
      pick = null;
      ghName = '';
      await runtimesStore.load(true);
    } catch (e) {
      ghError = runtimeInstallErrorText(e);
    } finally {
      if (timer) clearInterval(timer);
      timer = null;
      progress = null;
      installing = false;
    }
  }

  function fillPrism() {
    family = 'llama.cpp';
    repo = 'PrismML-Eng/llama.cpp';
    ghName = 'PrismML llama.cpp (Bonsai)';
    pick = null;
  }

  function fillWhisper() {
    family = 'whisper.cpp';
    repo = 'ggml-org/whisper.cpp';
    ghName = 'whisper.cpp';
    pick = null;
  }

  function fillSdcpp() {
    family = 'stable-diffusion.cpp';
    repo = 'leejet/stable-diffusion.cpp';
    ghName = 'stable-diffusion.cpp';
    pick = null;
  }

  function fillSherpa() {
    family = 'sherpa-onnx';
    repo = 'k2-fsa/sherpa-onnx';
    ghName = 'sherpa-onnx';
    pick = null;
  }

  // --- add a local folder --------------------------------------------------
  let folder = $state('');
  let localName = $state('');
  let copy = $state(false);
  let localFamily = $state('llama.cpp');
  let adding = $state(false);
  let localError = $state<string | null>(null);

  /** mistral.rs has no managed download: the user's own build folder (`target/release`). */
  function fillMistralrs() {
    localFamily = 'mistral.rs';
    if (!localName.trim()) localName = 'mistral.rs';
  }

  async function browse() {
    const picked = await invoke<string | null>('pick_directory_dialog').catch(() => null);
    if (picked) folder = picked;
  }

  async function addLocal() {
    adding = true;
    localError = null;
    try {
      const entry = await invoke<RuntimeEntry>('add_runtime_local', {
        request: { path: folder.trim(), name: localName.trim(), id: null, copy, family: localFamily },
      });
      toast.success(t('rt.added', { name: entry.name }));
      folder = '';
      localName = '';
      copy = false;
      await runtimesStore.load(true);
    } catch (e) {
      localError = localizeError(e);
    } finally {
      adding = false;
    }
  }
</script>

<SettingGroup title={t('rt.group_title')} hint={t('rt.group_desc')}>
  <div class="flex flex-col divide-y divide-border">
    {#if extra.length === 0}
      <p class="px-4 py-3 text-xs text-muted-foreground">{runtimesStore.loading ? '…' : t('rt.empty')}</p>
    {/if}
    {#each extra as r (r.id)}
      {@const report = reports[r.id]}
      <div class="flex flex-col gap-1.5 px-4 py-3">
        <div class="flex min-w-0 items-center justify-between gap-3">
          <div class="flex min-w-0 flex-col gap-0.5">
            <span class="flex min-w-0 items-center gap-1.5 text-sm font-medium">
              <span class="truncate">{r.name || r.id}</span>
              <Badge variant="outline" class="font-mono text-xs">{r.id}</Badge>
              <Badge variant="secondary" class="text-xs">{kindLabel(r)}</Badge>
            </span>
            <span class="truncate font-mono text-xs text-muted-foreground" title={sourceText(r.entry)}>
              {runtimeBuildLabel(r) || r.entry?.version || '?'} · {sourceText(r.entry)}
            </span>
          </div>
          <div class="flex shrink-0 items-center gap-1.5">
            <Button variant="outline" size="sm" disabled={busyId === r.id} onclick={() => verify(r.id)}>
              {busyId === r.id ? t('rt.verifying') : t('rt.verify')}
            </Button>
            <Button variant="ghost" size="sm" class="text-destructive" disabled={busyId === r.id} onclick={() => askRemove(r.id)}>
              {t('rt.remove')}
            </Button>
          </div>
        </div>
        <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
          {#if r.entry}
            <span class={cn('flex items-center gap-1', r.entry.trust === 'user_confirmed' && 'text-warning')}>
              <ShieldCheck class="size-3" />
              {t(`rt.trust_${r.entry.trust}` as TKey)}
            </span>
          {/if}
          {#if r.available}
            <span>{t('rt.summary', { flags: r.flag_count })}</span>
            <span>{r.unsupported_params.length > 0 ? t('rt.hidden_params', { count: r.unsupported_params.length }) : t('rt.hidden_none')}</span>
            {#if r.quant_types.length > 0}<span title={r.quant_types.join(', ')}>{t('rt.quants', { count: r.quant_types.length })}</span>{/if}
          {:else}
            <span class="text-destructive">{t('rt.unavailable', { error: localizeError(r.error ?? '') })}</span>
          {/if}
        </div>
        {#if report}
          <div
            class={cn(
              'flex flex-col gap-0.5 rounded-md border px-2.5 py-1.5 text-xs',
              report.ok ? 'border-success/30 bg-success/10' : 'border-destructive/30 bg-destructive/10',
            )}
            role={report.ok ? 'status' : 'alert'}
          >
            <span class="flex items-center gap-1.5">
              {#if report.ok}<Check class="size-3.5 text-success" />{:else}<TriangleAlert class="size-3.5 text-destructive" />{/if}
              {report.ok
                ? report.accepted
                  ? t('rt.verify_accepted', { version: report.version ?? '' })
                  : t('rt.verify_ok', { version: report.version ?? '' })
                : t('rt.verify_failed')}
            </span>
            {#if report.changed.length}<span class="font-mono text-xs">{t('rt.verify_changed', { list: report.changed.join(', ') })}</span>{/if}
            {#if report.missing.length}<span class="font-mono text-xs">{t('rt.verify_missing', { list: report.missing.join(', ') })}</span>{/if}
            {#if report.added.length}<span class="font-mono text-xs">{t('rt.verify_added', { list: report.added.join(', ') })}</span>{/if}
            {#if report.error}<span class="text-xs break-words">{localizeError(report.error)}</span>{/if}
            {#if !report.ok && r.entry?.source.kind === 'local'}
              <Button variant="outline" size="xs" class="self-start" disabled={busyId === r.id} onclick={() => verify(r.id, true)}>
                {t('rt.verify_accept')}
              </Button>
            {/if}
          </div>
        {/if}
        {#if removing?.id === r.id}
          <div class="flex flex-col gap-1.5 rounded-md border border-destructive/30 bg-destructive/5 px-2.5 py-2 text-xs" role="alert">
            <span>
              {t('rt.remove_confirm')}
              {r.entry?.source.kind === 'local' && !r.entry.source.copied ? t('rt.remove_local_note') : t('rt.remove_owned_note')}
            </span>
            {#if usersList(removing.users)}
              <span class="text-muted-foreground">{t('rt.remove_used', { list: usersList(removing.users) })}</span>
            {/if}
            <div class="flex gap-1.5">
              <Button variant="destructive" size="xs" disabled={busyId === r.id} onclick={() => remove(r, !!usersList(removing?.users ?? null))}>
                {usersList(removing.users) ? t('rt.remove_force') : t('rt.remove')}
              </Button>
              <Button variant="ghost" size="xs" onclick={() => (removing = null)}>{t('settings.cancel')}</Button>
            </div>
          </div>
        {/if}
      </div>
    {/each}

    <!-- Add from a GitHub release -->
    <div class="flex flex-col gap-2 px-4 py-3">
      <div class="flex flex-col gap-0.5">
        <span class="text-sm font-medium">{t('rt.add_github')}</span>
        <HelpPopover text={t('rt.add_github_hint')} />
      </div>
      <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/30 px-2.5 py-2 text-xs">
        <span class="font-medium">{t('rt.bonsai_title')}</span>
        <span class="text-muted-foreground">{t('rt.bonsai_desc')}</span>
        <Button variant="outline" size="xs" class="self-start" onclick={fillPrism}>{t('rt.bonsai_fill')}</Button>
      </div>
      <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/30 px-2.5 py-2 text-xs">
        <span class="font-medium">{t('rt.whisper_title')}</span>
        <span class="text-muted-foreground">{t('rt.whisper_desc')}</span>
        <Button variant="outline" size="xs" class="self-start" onclick={fillWhisper}>{t('rt.whisper_fill')}</Button>
      </div>
      <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/30 px-2.5 py-2 text-xs">
        <span class="font-medium">{t('rt.sdcpp_title')}</span>
        <span class="text-muted-foreground">{t('rt.sdcpp_desc')}</span>
        <Button variant="outline" size="xs" class="self-start" onclick={fillSdcpp}>{t('rt.sdcpp_fill')}</Button>
      </div>
      <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/30 px-2.5 py-2 text-xs">
        <span class="font-medium">{t('rt.sherpa_title')}</span>
        <span class="text-muted-foreground">{t('rt.sherpa_desc')}</span>
        <Button variant="outline" size="xs" class="self-start" onclick={fillSherpa}>{t('rt.sherpa_fill')}</Button>
      </div>
      <div class="grid grid-cols-2 gap-2 max-[900px]:grid-cols-1">
        <label class="flex flex-col gap-1 text-xs" for="{uid}-repo">
          {t('rt.repo')}
          <FieldHelp text={t('help.runtime_repo')}><Input id="{uid}-repo" class="h-8 font-mono text-xs" placeholder="PrismML-Eng/llama.cpp" bind:value={repo} /></FieldHelp>
        </label>
        <label class="flex flex-col gap-1 text-xs" for="{uid}-tag">
          {t('rt.tag')}
          <FieldHelp text={t('help.runtime_tag')}><Input id="{uid}-tag" class="h-8 font-num text-xs" placeholder={t('rt.tag_placeholder')} bind:value={tag} /></FieldHelp>
        </label>
        <label class="flex flex-col gap-1 text-xs" for="{uid}-backend">
          {t('rt.backend')}
          <FieldHelp text={t('help.runtime_backend')}><select
            id="{uid}-backend"
            class="h-8 rounded-lg border border-input bg-transparent px-2 text-sm dark:bg-input/30 [&>option]:bg-popover"
            bind:value={backend}
          >
            <option value="">{t('rt.backend_auto')}</option>
            {#each BACKENDS as b (b)}<option value={b}>{b}</option>{/each}
          </select></FieldHelp>
          {#if backend === 'rocm' && backendStatus?.rocm_unsupported_gpu}
            <span class="text-warning" role="status">
              {t('settings.backend_rocm_unsupported', { gpu: backendStatus.rocm_unsupported_gpu })}
            </span>
          {/if}
        </label>
        <label class="flex flex-col gap-1 text-xs" for="{uid}-ghname">
          {t('rt.name')}
          <FieldHelp text={t('help.runtime_name')}><Input id="{uid}-ghname" class="h-8 text-xs" placeholder={t('rt.name_placeholder')} bind:value={ghName} /></FieldHelp>
        </label>
      </div>
      <div class="flex gap-2">
        <Button variant="outline" size="sm" disabled={!repo.trim() || finding || installing} onclick={findRelease}>
          {finding ? t('rt.finding') : t('rt.find')}
        </Button>
      </div>
      {#if pick}
        <div class="flex flex-col gap-1.5 rounded-md border border-border px-2.5 py-2 text-xs" role="status">
          <span class="font-medium">{t('rt.pick_title', { repo: pick.repo, tag: pick.tag })}</span>
          {#each pick.assets as a (a.name)}
            <div class="flex min-w-0 flex-col gap-0.5">
              <span class="flex items-center gap-2 font-mono text-xs">
                <span class="truncate">{a.name}</span>
                <span class="shrink-0 font-num text-muted-foreground">{fmt(a.size)}</span>
              </span>
              <span class={cn('truncate font-mono text-xs', pick.unverified.includes(a.name) ? 'text-warning' : 'text-muted-foreground')} title={a.digest ?? ''}>
                {pick.unverified.includes(a.name) ? t('rt.no_digest') : `${t('rt.digest_ok')}: ${a.digest}`}
              </span>
              <span class="truncate font-mono text-xs text-muted-foreground" title={a.download_url}>{a.download_url}</span>
            </div>
          {/each}
          {#if pick.unverified.length > 0}
            <label class="flex items-start gap-2 text-warning" for="{uid}-unverified">
              <FieldHelp text={t('rt.accept_unverified')}><Checkbox id="{uid}-unverified" bind:checked={acceptUnverified} class="mt-0.5" /></FieldHelp>
              {t('rt.accept_unverified')}
            </label>
          {/if}
          {#if progress}
            <Progress value={pct} aria-label={progress.current_asset} />
            <span class="font-num text-xs text-muted-foreground">
              {fmt(progress.downloaded_bytes)}{progress.total_bytes > 0 ? ` / ${fmt(progress.total_bytes)}` : ''}
            </span>
          {/if}
          <Button size="sm" class="self-start" disabled={installing || (pick.unverified.length > 0 && !acceptUnverified)} onclick={install}>
            {installing ? t('rt.installing') : t('rt.install', { size: fmt(pickSize) })}
          </Button>
        </div>
      {/if}
      {#if ghError}<p class="text-xs break-words text-destructive" role="alert">{ghError}</p>{/if}
    </div>

    <!-- Add a local folder -->
    <div class="flex flex-col gap-2 px-4 py-3">
      <div class="flex flex-col gap-0.5">
        <span class="text-sm font-medium">{t('rt.add_local')}</span>
        <HelpPopover text={t('rt.add_local_hint')} />
      </div>
      <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/30 px-2.5 py-2 text-xs">
        <span class="font-medium">{t('rt.mistralrs_title')}</span>
        <span class="text-muted-foreground">{t('rt.mistralrs_desc')}</span>
        <Button variant="outline" size="xs" class="self-start" onclick={fillMistralrs}>{t('rt.mistralrs_fill')}</Button>
      </div>
      <div class="grid grid-cols-2 gap-2 max-[900px]:grid-cols-1">
        <label class="flex flex-col gap-1 text-xs" for="{uid}-folder">
          {t('rt.folder')}
          <span class="flex gap-1.5">
            <FieldHelp text={t('help.runtime_folder')}><Input id="{uid}-folder" class="h-8 min-w-0 flex-1 font-mono text-xs" placeholder={pathExample('C:\\llama.cpp\\build\\bin', '~/llama.cpp/build/bin')} bind:value={folder} /></FieldHelp>
            <Button variant="outline" size="sm" onclick={browse}><FolderOpen />{t('rt.browse')}</Button>
          </span>
        </label>
        <label class="flex flex-col gap-1 text-xs" for="{uid}-lname">
          {t('rt.name')}
          <FieldHelp text={t('help.runtime_name')}><Input id="{uid}-lname" class="h-8 text-xs" bind:value={localName} /></FieldHelp>
        </label>
      </div>
      <label class="flex items-center gap-2 text-xs" for="{uid}-lfamily">
        {t('rt.family_label')}
        <FieldHelp text={t('help.runtime_family')}><select id="{uid}-lfamily" bind:value={localFamily} class="h-8 rounded-md border border-input bg-background px-2 text-xs">
          <option value="llama.cpp">llama.cpp</option>
          <option value="whisper.cpp">whisper.cpp</option>
          <option value="audio.cpp">audio.cpp</option>
          <option value="stable-diffusion.cpp">stable-diffusion.cpp</option>
          <option value="mistral.rs">mistral.rs</option>
        </select></FieldHelp>
      </label>
      <label class="flex items-start gap-2 text-xs" for="{uid}-copy">
        <FieldHelp text={t('rt.copy_hint')}><Checkbox id="{uid}-copy" bind:checked={copy} class="mt-0.5" /></FieldHelp>
        <span class="flex flex-col gap-0.5">
          {t('rt.copy')}
          <span class="text-muted-foreground">{t('rt.copy_hint')}</span>
        </span>
      </label>
      <Button variant="outline" size="sm" class="self-start" disabled={!folder.trim() || adding} onclick={addLocal}>
        {adding ? t('rt.adding') : t('rt.add')}
      </Button>
      {#if localError}<p class="text-xs break-words text-destructive" role="alert">{localError}</p>{/if}
    </div>
  </div>
</SettingGroup>
