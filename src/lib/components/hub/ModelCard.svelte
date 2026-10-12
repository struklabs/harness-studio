<script lang="ts">
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import { onWindowActive } from '$lib/windowActivity';
  import { invoke } from '$lib/ipc';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import DOMPurify from 'dompurify';
  import type { Action } from 'svelte/action';
  import { untrack } from 'svelte';
  import { toast } from 'svelte-sonner';
  import { t, locale } from '$lib/i18n.svelte';
  import type { HfAccess, HfModelCard } from '$lib/types';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import { Skeleton } from '$lib/components/ui/skeleton';
  import { cn } from '$lib/utils';
  import { externalLinkFromEvent, renderReadme } from './markdown';
  import { notRunnableKey, taskSourceKey } from './inference';
  import { ExternalLink } from '@lucide/svelte';

  interface Props {
    repoId: string;
    onAccessGranted?: (repoId: string) => Promise<void>;
    /** Gated value from the search result, used until the card arrives. */
    gatedHint?: string | null;
    /** Access check result for a gated repo (null while unknown / not gated). */
    access?: HfAccess | null;
    /** Effective gated state ("auto" | "manual" | "true"), null when open. */
    gated?: string | null;
    /** True while the access check is running. */
    checkingAccess?: boolean;
    /** License id of the loaded card (null until it arrives / when unknown). */
    license?: string | null;
  }

  let {
    repoId,
    onAccessGranted,
    gatedHint = null,
    access = $bindable(null),
    gated = $bindable(null),
    checkingAccess = $bindable(false),
    license = $bindable(null),
  }: Props = $props();

  // Unique per instance: the README toggle references its body by id.
  const uid = $props.id();
  const readmeId = `readme-body-${uid}`;

  let card = $state<HfModelCard | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let readmeExpanded = $state(false);
  let token = 0;
  let awaitingLicenseReturn = false;
  let lastAccessCheck = 0;
  let returnTimer: ReturnType<typeof setTimeout> | undefined;

  function lacksAccess() {
    return access?.has_access === false || access?.reason?.includes('gated_no_access') === true;
  }

  export function markLicensePageOpened() {
    if (lacksAccess()) awaitingLicenseReturn = true;
  }

  function onReturn() {
    if (!awaitingLicenseReturn || !lacksAccess() || checkingAccess) return;
    clearTimeout(returnTimer);
    const remaining = 3_000 - (Date.now() - lastAccessCheck);
    if (remaining > 0) {
      returnTimer = setTimeout(onReturn, remaining);
      return;
    }
    awaitingLicenseReturn = false;
    void recheckAccess();
  }

  $effect(() => {
    const cleanup = onWindowActive(onReturn);
    window.addEventListener('focus', onReturn);
    return () => {
      cleanup();
      window.removeEventListener('focus', onReturn);
      clearTimeout(returnTimer);
    };
  });

  $effect(() => {
    const id = repoId;
    untrack(() => load(id));
  });

  async function load(id: string) {
    const my = ++token;
    awaitingLicenseReturn = false;
    clearTimeout(returnTimer);
    lastAccessCheck = 0;
    checkingAccess = false;
    card = null;
    error = null;
    access = null;
    license = null;
    gated = gatedHint;
    readmeExpanded = false;
    loading = true;
    // Gated per the search result: check access right away, in parallel with the card.
    const accessPromise = gatedHint ? checkAccess(id, my) : null;
    try {
      const res = await invoke<HfModelCard>('get_hf_model_card', { repoId: id });
      if (my !== token) return;
      card = res;
      license = res.license;
      gated = res.gated ?? gatedHint;
      if (res.gated && !accessPromise) void checkAccess(id, my);
    } catch (e) {
      if (my === token) error = t('hub.card_load_failed', { error: String(e) });
    } finally {
      if (my === token) loading = false;
    }
  }

  /** Checks access again (after the user accepted the license on huggingface.co). */
  export async function recheckAccess() {
    awaitingLicenseReturn = false;
    clearTimeout(returnTimer);
    await checkAccess(repoId, token);
  }

  async function checkAccess(id: string, my: number) {
    if (my === token && checkingAccess) return;
    const previouslyDenied = lacksAccess();
    checkingAccess = true;
    lastAccessCheck = Date.now();
    try {
      const res = await invoke<HfAccess>('check_hf_access', { repoId: id });
      if (my !== token) return;
      if (previouslyDenied && res.has_access === true) {
        await onAccessGranted?.(id);
      }
      if (my === token) access = res;
    } catch {
      if (my === token) access = null;
    } finally {
      if (my === token) {
        checkingAccess = false;
        lastAccessCheck = Date.now();
      }
    }
  }

  let readmeHtml = $derived(card?.readme ? renderReadme(card.readme, card.repo_id, DOMPurify) : '');

  function open(url: string) {
    if (url === `https://huggingface.co/${repoId}` || url.startsWith(`https://huggingface.co/${repoId}/`)) {
      markLicensePageOpened();
    }
    openUrl(url).catch((e) => toast.error(t('hub.open_link_failed', { error: String(e) })));
  }

  /** Opens README links in the system browser instead of navigating the webview. */
  const externalLinks: Action<HTMLElement> = (node) => {
    const onClick = (e: MouseEvent) => {
      const anchor = (e.target as Element | null)?.closest?.('a');
      if (!anchor) return;
      e.preventDefault();
      const url = externalLinkFromEvent(e);
      if (url) open(url);
    };
    node.addEventListener('click', onClick);
    return { destroy: () => node.removeEventListener('click', onClick) };
  };

  function gatedTitle(value: string | null): string {
    return value === 'manual' ? t('hub.gated_manual_title') : t('hub.gated_auto_title');
  }

  // Rendered README elements are not ours to class individually; style them from the container.
  const README_STYLES =
    'text-sm leading-relaxed break-words [&_h1]:mt-4 [&_h1]:mb-2 [&_h1]:text-base [&_h1]:font-semibold [&_h2]:mt-4 [&_h2]:mb-2 [&_h2]:text-base [&_h2]:font-semibold [&_h3]:mt-3 [&_h3]:mb-1.5 [&_h3]:text-sm [&_h3]:font-semibold [&_h4]:mt-3 [&_h4]:mb-1.5 [&_h4]:text-sm [&_h4]:font-medium [&_p]:my-2 [&_ul]:my-2 [&_ul]:list-disc [&_ul]:pl-5 [&_ol]:my-2 [&_ol]:list-decimal [&_ol]:pl-5 [&_a]:text-primary [&_a]:underline-offset-2 hover:[&_a]:underline [&_code]:rounded [&_code]:bg-muted [&_code]:px-1 [&_code]:font-mono [&_code]:text-xs [&_pre]:my-2 [&_pre]:overflow-x-auto [&_pre]:rounded-md [&_pre]:border [&_pre]:border-border [&_pre]:bg-muted [&_pre]:p-2.5 [&_pre_code]:bg-transparent [&_pre_code]:p-0 [&_table]:my-2 [&_table]:block [&_table]:max-w-full [&_table]:overflow-x-auto [&_table]:border-collapse [&_table]:text-xs [&_th]:border [&_th]:border-border [&_th]:px-2 [&_th]:py-1 [&_th]:text-left [&_td]:border [&_td]:border-border [&_td]:px-2 [&_td]:py-1 [&_blockquote]:my-2 [&_blockquote]:border-l-2 [&_blockquote]:border-border [&_blockquote]:pl-3 [&_blockquote]:text-muted-foreground [&_img]:max-w-full [&_.md-img-alt]:text-muted-foreground [&_.md-img-alt]:italic';
</script>

<section class="flex min-w-0 flex-col gap-4" aria-busy={loading}>
  {#if loading}
    <div class="flex flex-col gap-2" role="status" aria-label={t('hub.card_loading')}>
      <Skeleton class="h-4 w-1/3" />
      <Skeleton class="h-16 w-full" />
      <Skeleton class="h-32 w-full" />
    </div>
  {:else if error}
    <div class="flex items-center justify-between gap-3 rounded-lg border border-destructive/30 bg-destructive/10 p-3 text-xs text-destructive" role="alert">
      <span class="min-w-0 break-words">{error}</span>
      <Button variant="outline" size="xs" onclick={() => load(repoId)}>{t('hub.retry_button')}</Button>
    </div>
  {:else if card}
    <dl class="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-x-4 gap-y-3 text-xs">
      <div class="min-w-0">
        <dt class="text-xs text-muted-foreground">{t('hub.card_license')}</dt>
        <dd class="truncate font-mono">{card.license ?? t('hub.card_unknown')}</dd>
      </div>
      <div class="min-w-0">
        <dt class="text-xs text-muted-foreground">{t('hub.card_access')}</dt>
        <dd>
          {#if gated}
            <Badge variant="outline" class="text-warning" title={gatedTitle(gated)}>{t('hfauth.gate_title')}</Badge>
          {:else}
            {t('hub.card_open_access')}
          {/if}
        </dd>
      </div>
      <div class="min-w-0">
        <dt class="text-xs text-muted-foreground">{t('hub.card_task')}</dt>
        <dd class="truncate" title={card.pipeline_tag ? undefined : t(taskSourceKey(card.inferred_source), { task: card.inferred_task ?? '' })}>
          {card.pipeline_tag ?? (card.inferred_task ? t('hub.card_task_inferred', { task: card.inferred_task }) : t('hub.card_unknown'))}
        </dd>
      </div>
      {#if card.not_runnable === 'adapter'}
        <div class="col-span-full min-w-0 rounded-md border border-info/30 bg-info/10 px-2.5 py-1.5 text-info" role="note">
          <span class="font-medium">{t('hub.badge_lora')}.</span>
          {card.adapter_base ? t('hub.badge_lora_title', { base: card.adapter_base }) : t('hub.badge_lora_title_nobase')}
        </div>
      {:else if card.not_runnable}
        <div class="col-span-full min-w-0 rounded-md border border-warning/30 bg-warning/10 px-2.5 py-1.5 text-warning" role="note">
          <span class="font-medium">{t('hub.not_runnable')}.</span>
          {t(notRunnableKey(card.not_runnable))}
        </div>
      {/if}
      <div class="min-w-0">
        <dt class="text-xs text-muted-foreground">{t('hub.card_stats')}</dt>
        <dd class="truncate font-num">
          {card.downloads.toLocaleString(locale())} · {card.likes.toLocaleString(locale())}
        </dd>
      </div>
      {#if card.last_modified}
        <div class="min-w-0">
          <dt class="text-xs text-muted-foreground">{t('hub.card_updated')}</dt>
          <dd class="font-num">{new Date(card.last_modified).toLocaleDateString(locale())}</dd>
        </div>
      {/if}
      {#if card.base_model.length > 0}
        <div class="col-span-full min-w-0">
          <dt class="text-xs text-muted-foreground">{t('hub.card_base_model')}</dt>
          <dd class="flex flex-wrap gap-x-3 gap-y-1">
            {#each card.base_model as base (base)}
              <button
                type="button"
                class="inline-flex items-center gap-1 font-mono text-primary hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none"
                onclick={() => open(`https://huggingface.co/${base}`)}
              >
                <span class="truncate">{base}</span>
                <ExternalLink class="size-3 shrink-0" />
              </button>
            {/each}
          </dd>
        </div>
      {/if}
    </dl>

    {#if card.tags.length > 0}
      <!-- Hugging Face tags are many and mostly technical: listed on demand, as plain text. -->
      <AdvancedOnly>
        <details class="text-xs">
          <summary class="w-fit cursor-pointer rounded-sm text-muted-foreground outline-none select-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50">
            {t('hub.card_tags')} ({card.tags.length})
          </summary>
          <p class="mt-1 font-mono break-words text-muted-foreground">{card.tags.join(', ')}</p>
        </details>
      </AdvancedOnly>
    {/if}

    <div class="flex flex-col gap-2">
      <h3 class="text-sm font-semibold text-foreground">{t('hub.card_readme')}</h3>
      {#if readmeHtml}
        <div
          id={readmeId}
          class={cn(
            'relative min-w-0 overflow-hidden rounded-lg border border-border bg-card p-3',
            README_STYLES,
            !readmeExpanded && 'max-h-64'
          )}
          use:externalLinks
        >
          <!-- Sanitized by DOMPurify in renderReadme -->
          {@html readmeHtml}
          {#if !readmeExpanded}
            <div class="pointer-events-none absolute inset-x-0 bottom-0 h-12 bg-gradient-to-t from-card to-transparent"></div>
          {/if}
        </div>
        <Button
          variant="outline"
          size="xs"
          class="self-start"
          aria-expanded={readmeExpanded}
          aria-controls={readmeId}
          onclick={() => (readmeExpanded = !readmeExpanded)}
        >
          {readmeExpanded ? t('hub.readme_show_less') : t('hub.readme_show_more')}
        </Button>
      {:else}
        <p class="text-xs text-muted-foreground">{t('hub.readme_empty')}</p>
      {/if}
    </div>
  {/if}
</section>
