<script lang="ts">
  // A ```mermaid block. The source is shown until the fence is closed; then, once the block is
  // on screen and has stopped changing for a moment, it is rendered as a diagram (Mermaid is
  // loaded lazily, see ./mermaid.ts). Re-rendered when the app theme changes.
  import { onDestroy } from 'svelte';
  import { toast } from 'svelte-sonner';
  import { t } from '#lib/i18n.svelte.js';
  import { onSchemeChange } from '#lib/theme.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Check, Code, Copy, Download, Workflow } from '@lucide/svelte';
  import { diagramBackground, renderMermaid } from './mermaid';
  import { svgToPng } from './diagramPng';
  import { convApi } from './conversations';
  import { scrollFocusable } from '#lib/actions/scrollFocusable.js';

  interface Props {
    code: string;
    /** The closing fence has arrived; before that the source is still streaming. */
    complete: boolean;
  }

  let { code, complete }: Props = $props();

  const DEBOUNCE_MS = 400;

  let view = $state<'diagram' | 'code'>('diagram');
  let svg = $state<string | null>(null);
  let error = $state<string | null>(null);
  let rendering = $state(false);
  /** Code + theme generation the current `svg`/`error` belongs to. */
  let renderedKey = '';
  let themeGen = $state(0);
  let visible = $state(false);
  let copied = $state(false);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;
  let host = $state<HTMLElement | null>(null);

  const unScheme = onSchemeChange(() => themeGen++);
  const contrastMq = typeof window !== 'undefined' && window.matchMedia ? window.matchMedia('(prefers-contrast: more)') : null;
  const onContrast = () => themeGen++;
  contrastMq?.addEventListener('change', onContrast);
  onDestroy(() => {
    unScheme();
    contrastMq?.removeEventListener('change', onContrast);
    clearTimeout(copyTimer);
  });

  // Render only what is (nearly) on screen: old conversations may hold many diagrams.
  $effect(() => {
    const el = host;
    if (!el || typeof IntersectionObserver === 'undefined') {
      visible = true;
      return;
    }
    const io = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) {
        visible = true;
        io.disconnect();
      }
    }, { rootMargin: '200px' });
    io.observe(el);
    return () => io.disconnect();
  });

  $effect(() => {
    const source = code.trim();
    const key = `${themeGen}\u0000${source}`;
    if (!complete || !visible || view !== 'diagram' || !source || key === renderedKey) return;
    const timer = setTimeout(async () => {
      rendering = true;
      const result = await renderMermaid(source);
      rendering = false;
      // A newer code/theme may have arrived while rendering; its own run will follow.
      if (`${themeGen}\u0000${code.trim()}` !== key) return;
      renderedKey = key;
      if (result.ok) {
        svg = result.svg;
        error = null;
      } else {
        svg = null;
        error = result.error;
      }
    }, DEBOUNCE_MS);
    return () => clearTimeout(timer);
  });

  let showDiagram = $derived(view === 'diagram' && complete && !!svg && !error);

  async function copySource() {
    try {
      await navigator.clipboard.writeText(code);
      copied = true;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copied = false), 1500);
    } catch {
      // clipboard unavailable
    }
  }

  async function exportPng() {
    if (!svg) return;
    try {
      const png = await svgToPng(svg, diagramBackground());
      const path = await convApi.exportDiagramPng(png, t('pg.mermaid_file_name'));
      if (path) toast.success(t('pg.mermaid_exported', { path }));
    } catch (e) {
      toast.error(t('pg.mermaid_png_failed', { message: e instanceof Error ? e.message : String(e) }));
    }
  }

  async function exportSvg() {
    if (!svg) return;
    try {
      const doc = svg.replace(/^<svg(?![^>]*\bxmlns=)/, '<svg xmlns="http://www.w3.org/2000/svg"');
      const path = await convApi.exportDiagramSvg(`<?xml version="1.0" encoding="UTF-8"?>\n${doc}`, t('pg.mermaid_file_name'));
      if (path) toast.success(t('pg.mermaid_exported', { path }));
    } catch (e) {
      toast.error(String(e));
    }
  }
</script>

<div bind:this={host} class="overflow-hidden rounded-lg border border-border bg-card">
  <div class="flex flex-wrap items-center gap-1 border-b border-border bg-muted/50 py-0.5 pr-1 pl-3">
    <span class="font-mono text-xs text-muted-foreground lowercase">mermaid</span>
    {#if complete && rendering && view === 'diagram' && !svg && !error}
      <span class="text-xs text-muted-foreground" role="status">{t('pg.mermaid_rendering')}</span>
    {/if}
    <div class="ml-auto flex items-center gap-0.5">
      {#if complete}
        <div class="flex items-center rounded-md border border-border p-px" role="group" aria-label={t('pg.mermaid_view')}>
          <Button
            variant={view === 'diagram' ? 'secondary' : 'ghost'}
            size="xs"
            aria-pressed={view === 'diagram'}
            onclick={() => (view = 'diagram')}
          >
            <Workflow />{t('pg.mermaid_diagram')}
          </Button>
          <Button variant={view === 'code' ? 'secondary' : 'ghost'} size="xs" aria-pressed={view === 'code'} onclick={() => (view = 'code')}>
            <Code />{t('pg.mermaid_code')}
          </Button>
        </div>
      {/if}
      {#if showDiagram}
        <Button variant="ghost" size="xs" aria-label={t('pg.mermaid_export_svg')} title={t('pg.mermaid_export_svg')} onclick={exportSvg}>
          <Download />SVG
        </Button>
        <Button variant="ghost" size="xs" aria-label={t('pg.mermaid_export_png')} title={t('pg.mermaid_export_png')} onclick={exportPng}>
          <Download />PNG
        </Button>
      {/if}
      <Button variant="ghost" size="xs" aria-label={t('pg.copy_code_aria', { lang: 'mermaid' })} onclick={copySource}>
        {#if copied}<Check />{:else}<Copy />{/if}
        {copied ? t('pg.code_copied') : t('pg.copy_code')}
      </Button>
    </div>
  </div>
  {#if showDiagram}
    <div class="flex justify-center overflow-x-auto p-3 text-foreground [&_svg]:h-auto [&_svg]:max-w-full" role="figure" aria-label={t('pg.mermaid_diagram_aria')}>
      <!-- Mermaid output (securityLevel strict, htmlLabels off), sanitized again by DOMPurify in ./mermaid.ts -->
      {@html svg}
    </div>
  {:else}
    {#if view === 'diagram' && complete && error}
      <p class="border-b border-border px-3 py-1.5 text-xs text-destructive" role="alert">
        {t('pg.mermaid_error')}
        <span class="block font-mono text-muted-foreground [overflow-wrap:anywhere]">{error}</span>
      </p>
    {/if}
    <pre use:scrollFocusable class="m-0 overflow-x-auto p-3 font-mono text-xs leading-relaxed text-foreground"><code>{code}</code></pre>
  {/if}
</div>
