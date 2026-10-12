<script lang="ts">
  // Settings > Runtime > ONNX Runtime: the libraries harness-onnx loads (ONNX Runtime and, on Windows, GenAI: chat, embeddings / rerank for
  // Hugging Face ONNX models). Downloaded on demand, every file checked against a pinned SHA-256.
  import { invoke } from '$lib/ipc';
  import { onDestroy, onMount } from 'svelte';
  import { toast } from 'svelte-sonner';
  import type { OnnxRuntimeProgress, OnnxRuntimeSupport, OnnxRuntimeVariant } from '$lib/types';
  import { t, type TKey } from '$lib/i18n.svelte';
  import { formatBytes } from '$lib/format';
  import { onnxUnsupportedText } from '$lib/engines';
  import { localizeError } from '$lib/network';
  import { Button } from '$lib/components/ui/button';
  import { Badge } from '$lib/components/ui/badge';
  import { Progress } from '$lib/components/ui/progress';
  import { ShieldCheck } from '@lucide/svelte';
  import SettingGroup from './SettingGroup.svelte';

  const fmt = (b: number) => formatBytes(b, 1);

  let variants = $state<OnnxRuntimeVariant[]>([]);
  /** No build for this OS / architecture at all (Intel Mac): the card says so instead of offering installs. */
  let unsupported = $state<OnnxRuntimeSupport | null>(null);
  let busy = $state<string | null>(null);
  let progress = $state<OnnxRuntimeProgress | null>(null);
  let verified = $state<Record<string, string[]>>({});
  let timer: ReturnType<typeof setInterval> | null = null;
  onDestroy(() => timer && clearInterval(timer));

  let pct = $derived(progress && progress.total_bytes > 0 ? (progress.downloaded_bytes / progress.total_bytes) * 100 : 0);

  async function load() {
    try {
      const [list, support] = await Promise.all([
        invoke<OnnxRuntimeVariant[]>('list_onnx_runtime'),
        invoke<OnnxRuntimeSupport>('get_onnx_runtime_support'),
      ]);
      variants = list;
      unsupported = support.supported ? null : support;
    } catch (e) {
      toast.error(localizeError(e));
    }
  }
  onMount(load);

  async function install(v: string) {
    busy = v;
    timer = setInterval(async () => {
      try {
        progress = await invoke<OnnxRuntimeProgress | null>('get_onnx_runtime_progress');
      } catch {
        /* best effort */
      }
    }, 500);
    try {
      await invoke('install_onnx_runtime', { variant: v });
      toast.success(t('ort.installed_ok', { variant: v }));
      await load();
    } catch (e) {
      toast.error(localizeError(e));
    } finally {
      if (timer) clearInterval(timer);
      timer = null;
      progress = null;
      busy = null;
    }
  }

  async function verify(v: string) {
    busy = v;
    try {
      verified[v] = await invoke<string[]>('verify_onnx_runtime', { variant: v });
    } catch (e) {
      toast.error(localizeError(e));
    } finally {
      busy = null;
    }
  }

  async function remove(v: string) {
    busy = v;
    try {
      await invoke('remove_onnx_runtime', { variant: v });
      toast.success(t('ort.removed', { variant: v }));
      delete verified[v];
      await load();
    } catch (e) {
      toast.error(localizeError(e));
    } finally {
      busy = null;
    }
  }
</script>

{#if unsupported}
  {@const msg = onnxUnsupportedText(unsupported)}
  <SettingGroup title={t('ort.title')} hint={t('ort.hint')}>
    <p class="px-4 py-3 text-sm text-warning" role="status">{t(msg.key, msg.params)}</p>
  </SettingGroup>
{:else if variants.length > 0}
  <SettingGroup title={t('ort.title')} hint={t('ort.hint')}>
    <div class="flex flex-col divide-y divide-border">
      {#each variants as v (v.variant)}
        <div class="flex flex-col gap-1.5 px-4 py-3">
          <div class="flex min-w-0 items-center justify-between gap-3">
            <div class="flex min-w-0 flex-col gap-0.5">
              <span class="flex min-w-0 items-center gap-1.5 text-sm font-medium">
                <span class="truncate">{t(`ort.variant_${v.variant}` as TKey)}</span>
                <Badge variant="outline" class="font-mono text-xs">{v.version}</Badge>
                {#if v.default}<Badge variant="secondary" class="text-xs">{t('ort.default')}</Badge>{/if}
              </span>
              <span class="truncate font-mono text-xs text-muted-foreground" title={v.installed?.dir ?? ''}>
                {v.installed ? `${t('ort.installed')} · ${fmt(v.installed.size_bytes)} · ${v.installed.dir}` : t('ort.not_installed')}
              </span>
            </div>
            <div class="flex shrink-0 items-center gap-1.5">
              {#if v.installed}
                <Button variant="outline" size="sm" disabled={busy === v.variant} onclick={() => verify(v.variant)}>
                  {busy === v.variant ? t('ort.verifying') : t('ort.verify')}
                </Button>
                <Button variant="ghost" size="sm" class="text-destructive" disabled={busy === v.variant} onclick={() => remove(v.variant)}>
                  {t('ort.remove')}
                </Button>
              {:else}
                <Button size="sm" disabled={busy !== null} onclick={() => install(v.variant)}>
                  {busy === v.variant ? t('ort.installing') : t('ort.install', { size: fmt(v.download_bytes) })}
                </Button>
              {/if}
            </div>
          </div>
          {#if busy === v.variant && progress}
            <Progress value={pct} aria-label={progress.current} />
            <span class="font-num text-xs text-muted-foreground">
              {fmt(progress.downloaded_bytes)}{progress.total_bytes > 0 ? ` / ${fmt(progress.total_bytes)}` : ''}
            </span>
          {/if}
          {#if verified[v.variant]}
            {@const problems = verified[v.variant]}
            <span class="flex items-center gap-1.5 text-xs {problems.length ? 'text-destructive' : 'text-success'}" role={problems.length ? 'alert' : 'status'}>
              <ShieldCheck class="size-3.5" />
              {problems.length ? t('ort.verify_failed', { list: problems.join(', ') }) : t('ort.verify_ok')}
            </span>
          {/if}
        </div>
      {/each}
    </div>
  </SettingGroup>
{/if}
