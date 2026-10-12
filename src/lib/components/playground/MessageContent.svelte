<script lang="ts">
  // Renderer for model output. Fenced ``` blocks are split out first and become <pre><code>
  // with a copy button (an unterminated fence mid-stream renders as code); ```mermaid blocks
  // become diagrams once closed (MermaidBlock). Everything else is
  // markdown (headings, lists, tables, quotes, links, emphasis, inline code) rendered by
  // `marked` and sanitized by DOMPurify before it reaches {@html}.
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { toast } from 'svelte-sonner';
  import { t } from '$lib/i18n.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Check, Copy } from '@lucide/svelte';
  import { renderChatMarkdown } from './markdown';
  import MermaidBlock from './MermaidBlock.svelte';

  interface Props {
    text: string;
  }

  let { text }: Props = $props();

  type Segment = { kind: 'text'; html: string } | { kind: 'code'; lang: string; value: string; closed: boolean };

  const MD_CLASSES =
    'min-w-0 text-base leading-relaxed [overflow-wrap:anywhere] [&>*:first-child]:mt-0 [&>*:last-child]:mb-0 [&_p]:my-2 [&_h1]:mt-4 [&_h1]:mb-2 [&_h1]:text-lg [&_h1]:font-semibold [&_h2]:mt-4 [&_h2]:mb-2 [&_h2]:text-base [&_h2]:font-semibold [&_h3]:mt-3 [&_h3]:mb-1.5 [&_h3]:text-base [&_h3]:font-semibold [&_h4]:mt-3 [&_h4]:mb-1 [&_h4]:font-semibold [&_h5]:font-semibold [&_h6]:font-semibold [&_ul]:my-2 [&_ul]:list-disc [&_ul]:pl-5 [&_ol]:my-2 [&_ol]:list-decimal [&_ol]:pl-5 [&_li]:my-0.5 [&_li>p]:my-0.5 [&_a]:text-primary [&_a]:underline [&_a]:underline-offset-2 [&_strong]:font-semibold [&_em]:italic [&_del]:line-through [&_hr]:my-4 [&_hr]:border-border [&_code]:rounded [&_code]:bg-muted [&_code]:px-1 [&_code]:py-0.5 [&_code]:font-mono [&_code]:text-xs [&_pre]:my-2 [&_pre]:overflow-x-auto [&_pre]:rounded-lg [&_pre]:border [&_pre]:border-border [&_pre]:bg-card [&_pre]:p-3 [&_pre_code]:bg-transparent [&_pre_code]:p-0 [&_table]:my-2 [&_table]:block [&_table]:max-w-full [&_table]:overflow-x-auto [&_table]:border-collapse [&_table]:text-xs [&_th]:border [&_th]:border-border [&_th]:bg-muted/50 [&_th]:px-2 [&_th]:py-1 [&_th]:text-left [&_th]:font-semibold [&_th]:[overflow-wrap:break-word] [&_td]:border [&_td]:border-border [&_td]:px-2 [&_td]:py-1 [&_td]:align-top [&_td]:[overflow-wrap:break-word] [&_td]:min-w-[8ch] [&_blockquote]:my-2 [&_blockquote]:border-l-2 [&_blockquote]:border-border [&_blockquote]:pl-3 [&_blockquote]:text-muted-foreground [&_input]:mr-1';

  function textSegment(value: string): Segment | null {
    return value.trim() ? { kind: 'text', html: renderChatMarkdown(value) } : null;
  }

  /** Next ``` that starts a line (optionally indented), at or after `from`. */
  function findFence(src: string, from: number): number {
    let i = src.indexOf('```', from);
    while (i !== -1) {
      const lineStart = src.lastIndexOf('\n', i - 1) + 1;
      if (/^[ \t]*$/.test(src.slice(lineStart, i))) return i;
      i = src.indexOf('```', i + 3);
    }
    return -1;
  }

  function parse(src: string): Segment[] {
    const segments: Segment[] = [];
    const pushText = (value: string) => {
      const seg = textSegment(value);
      if (seg) segments.push(seg);
    };
    let pos = 0;
    while (pos < src.length) {
      const open = findFence(src, pos);
      if (open === -1) {
        pushText(src.slice(pos));
        break;
      }
      if (open > pos) pushText(src.slice(pos, open));
      const langEnd = src.indexOf('\n', open + 3);
      if (langEnd === -1) {
        // Opening fence still streaming in: show the (possibly partial) language as an empty block.
        segments.push({ kind: 'code', lang: src.slice(open + 3).trim(), value: '', closed: false });
        break;
      }
      const lang = src.slice(open + 3, langEnd).trim();
      const close = findFence(src, langEnd + 1);
      if (close === -1) {
        segments.push({ kind: 'code', lang, value: src.slice(langEnd + 1), closed: false });
        break;
      }
      segments.push({
        kind: 'code',
        lang,
        value: src.slice(langEnd + 1, close).replace(/\n[ \t]*$/, ''),
        closed: true,
      });
      pos = close + 3;
      // Skip the rest of the closing fence line.
      const eol = src.indexOf('\n', pos);
      pos = eol === -1 ? src.length : eol + 1;
    }
    return segments;
  }

  /** Links in rendered markdown open in the external browser; the webview never navigates. */
  function onContentClick(e: MouseEvent) {
    const a = (e.target as Element | null)?.closest?.('a');
    if (!a) return;
    e.preventDefault();
    const href = a.getAttribute('href');
    if (href && /^(https?:|mailto:)/i.test(href)) {
      openUrl(href).catch((err) => toast.error(t('pg.open_link_failed', { error: String(err) })));
    }
  }

  /** ```mermaid (the info string's first word, any case). */
  function isMermaid(lang: string) {
    return lang.split(/\s/, 1)[0].toLowerCase() === 'mermaid';
  }

  let segments = $derived(parse(text));
  let copiedIndex = $state<number | null>(null);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  async function copyCode(value: string, index: number) {
    try {
      await navigator.clipboard.writeText(value);
      copiedIndex = index;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copiedIndex = null), 1500);
    } catch {
      // clipboard unavailable
    }
  }

  $effect(() => () => clearTimeout(copyTimer));
</script>

<div class="flex min-w-0 flex-col gap-3">
  {#each segments as seg, i (i)}
    {#if seg.kind === 'text'}
      <!-- Delegated link clicks; the links themselves stay keyboard-focusable. -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class={MD_CLASSES} onclick={onContentClick}>
        <!-- Sanitized by DOMPurify in renderChatMarkdown -->
        {@html seg.html}
      </div>
    {:else if isMermaid(seg.lang)}
      <MermaidBlock code={seg.value} complete={seg.closed} />
    {:else}
      {@const lang = seg.lang || t('pg.code_label')}
      <div class="overflow-hidden rounded-lg border border-border bg-card">
        <div class="flex items-center justify-between border-b border-border bg-muted/50 py-0.5 pr-1 pl-3">
          <span class="font-mono text-xs text-muted-foreground lowercase">{lang}</span>
          <Button variant="ghost" size="xs" aria-label={t('pg.copy_code_aria', { lang })} onclick={() => copyCode(seg.value, i)}>
            {#if copiedIndex === i}<Check />{:else}<Copy />{/if}
            {copiedIndex === i ? t('pg.code_copied') : t('pg.copy_code')}
          </Button>
        </div>
        <pre class="m-0 overflow-x-auto p-3 font-mono text-xs leading-relaxed text-foreground"><code>{seg.value}</code></pre>
      </div>
    {/if}
  {/each}
</div>
