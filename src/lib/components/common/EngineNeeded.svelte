<script lang="ts">
  // "This model needs the {runtime} engine" with its one-click install button. After a
  // successful install the whole notice goes away and `onInstalled` runs, so the model can be
  // loaded or used at once. A family without a managed download says how to add it instead.
  import { LoaderCircle, Download } from '@lucide/svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { engineInstall } from '#lib/stores/engineInstall.svelte.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { security } from '#lib/stores/security.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { cn } from '#lib/utils.js';

  interface Props {
    /** Engine family as the backend names it (`whisper.cpp`). */
    family: string;
    /** Show the "needs {runtime}" sentence in front of the button. */
    hint?: boolean;
    hintText?: string;
    size?: 'xs' | 'sm';
    class?: string;
    onInstalled?: () => void;
  }
  let { family, hint = true, hintText, size = 'xs', class: className, onInstalled }: Props = $props();

  let st = $derived(engineInstall.state(family));
  let manual = $derived(!engineInstall.canInstall(family));

  async function install() {
    if (await engineInstall.install(family)) {
      onInstalled?.();
    }
  }
</script>

{#if !engineInstall.installed(family) || (family === 'audio.cpp' && !app.settings?.audio_cpp_enabled)}
  <span class={cn('inline-flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1', className)}>
    {#if family === 'audio.cpp' && !app.settings?.audio_cpp_enabled}
      <span>{t('audio_cpp.needs_engine')}</span>
      <Button type="button" variant="outline" {size} onclick={() => security.openSettings('runtime-installed')}>{t('audio_cpp.experimental')}</Button>
    {:else}
    {#if hint}<span class="min-w-0 flex-1">{hintText ?? t('lib.runtime_needs_hint', { runtime: family })}</span>{/if}
    {#if manual}
      <span class="text-muted-foreground">{t('eng.manual', { runtime: family })}</span>
    {:else}
      <Button type="button" variant="outline" {size} class="shrink-0" disabled={engineInstall.anyBusy} onclick={install}>
        {#if st.busy}<LoaderCircle class="animate-spin" />{:else}<Download />{/if}
        {st.busy ? t(st.pct === null ? 'eng.installing' : 'eng.installing_pct', { runtime: family, pct: st.pct ?? 0 }) : t('eng.install', { runtime: family })}
      </Button>
    {/if}
    {#if st.problem === 'unverified'}
      <span class="min-w-0 basis-full text-warning" role="alert">{t('eng.unverified', { runtime: family })}</span>
    {:else if st.problem === 'failed' && st.error}
      <details class="min-w-0 basis-full text-destructive" role="alert">
        <summary class="cursor-pointer [overflow-wrap:anywhere]">{t('eng.failed_summary', { runtime: family })}</summary>
        <pre class="mt-2 max-h-60 overflow-auto whitespace-pre-wrap [overflow-wrap:anywhere]">{st.error}</pre>
      </details>
    {/if}
    {#if uiLevel.advanced && (manual || st.problem === 'unverified')}
      <Button type="button" variant="link" {size} class="h-auto p-0" onclick={() => security.openSettings('runtime-registry')}>
        {t('eng.open_registry')}
      </Button>
    {/if}
    {/if}
  </span>
{/if}
