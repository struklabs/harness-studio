<script lang="ts">
  import { marked } from 'marked';
  import DOMPurify from 'dompurify';
  import { Button } from '#lib/components/ui/button/index.js';
  import { t } from '#lib/i18n.svelte.js';
  import { bytes } from '../text.ts';

  // Read-only viewing of files the user picks (the iOS document picker): text, Markdown, JSON
  // and images. Nothing is uploaded; the files stay in this screen's memory.
  interface Viewed {
    name: string;
    size: number;
    kind: 'text' | 'markdown' | 'json' | 'image' | 'other';
    text?: string;
    url?: string;
  }

  const MAX_TEXT = 2 * 1024 * 1024;
  let files = $state<Viewed[]>([]);
  let open = $state<Viewed | null>(null);

  function kindOf(f: File): Viewed['kind'] {
    if (f.type.startsWith('image/')) return 'image';
    if (/\.(md|markdown)$/i.test(f.name)) return 'markdown';
    if (/\.json$/i.test(f.name) || f.type === 'application/json') return 'json';
    if (f.type.startsWith('text/') || /\.(txt|log|csv|ts|js|py|rs|toml|yaml|yml|sh|html|css)$/i.test(f.name)) return 'text';
    return 'other';
  }

  async function add(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    for (const f of Array.from(input.files ?? [])) {
      const kind = kindOf(f);
      const item: Viewed = { name: f.name, size: f.size, kind };
      if (kind === 'image') item.url = URL.createObjectURL(f);
      else if (kind !== 'other' && f.size <= MAX_TEXT) item.text = await f.text();
      files = [item, ...files];
      open = item;
    }
    input.value = '';
  }

  function pretty(text: string): string {
    try {
      return JSON.stringify(JSON.parse(text), null, 2);
    } catch {
      return text;
    }
  }
</script>

<div class="space-y-3 p-4">
  <label class="block">
    <span class="sr-only">{t('mobile.files.pick')}</span>
    <input type="file" multiple class="block w-full text-sm file:mr-3 file:rounded-md file:border-0 file:bg-primary file:px-3 file:py-2 file:text-primary-foreground" onchange={add} />
  </label>
  <p class="text-xs text-muted-foreground">{t('mobile.files.hint')}</p>

  {#if open}
    <div class="rounded-lg border border-border bg-card">
      <div class="flex items-center justify-between border-b border-border p-2">
        <span class="truncate text-sm font-medium">{open.name}</span>
        <Button size="sm" variant="ghost" onclick={() => (open = null)}>{t('mobile.close')}</Button>
      </div>
      <div class="max-h-[60vh] overflow-auto p-3 text-sm">
        {#if open.kind === 'image' && open.url}
          <img src={open.url} alt={open.name} class="max-w-full" />
        {:else if open.text === undefined}
          <p class="text-muted-foreground">{t('mobile.files.no_preview', { size: bytes(open.size) })}</p>
        {:else if open.kind === 'markdown'}
          <div class="prose-sm">{@html DOMPurify.sanitize(marked.parse(open.text, { async: false }) as string)}</div>
        {:else}
          <pre class="font-mono text-xs whitespace-pre-wrap">{open.kind === 'json' ? pretty(open.text) : open.text}</pre>
        {/if}
      </div>
    </div>
  {/if}

  {#each files as f, i (i)}
    <button type="button" class="flex w-full items-center justify-between rounded-lg border border-border bg-card p-2 text-left text-sm" onclick={() => (open = f)}>
      <span class="truncate">{f.name}</span>
      <span class="text-xs text-muted-foreground">{bytes(f.size)}</span>
    </button>
  {/each}
</div>
