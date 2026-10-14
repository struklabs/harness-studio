<script lang="ts">
  // The resources of one connected MCP server (`resources/list` and templates). With `onAttach`
  // each resource gets an "Attach" button (the Playground adds it to the chat as a document), and
  // a field takes any URI, which is how a template's resource is reached.
  import { onDestroy, onMount } from 'svelte';
  import { FileText, Paperclip } from '@lucide/svelte';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { t } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { formatBytes } from '#lib/format.js';
  import { listMcpResources, type McpResourceList } from '#lib/tools-client.js';

  interface Props {
    server: string;
    /** Attach this resource (uri, display name); rejects with the reason it failed. */
    onAttach?: (uri: string, name: string) => Promise<void>;
  }

  let { server, onAttach }: Props = $props();

  let list = $state<McpResourceList | null>(null);
  let error = $state('');
  let loading = $state(true);
  let busy = $state<string | null>(null);
  let attachError = $state('');
  let uriInput = $state('');
  let alive = true;

  onDestroy(() => (alive = false));
  onMount(async () => {
    try {
      const l = await listMcpResources(server);
      if (alive) list = l;
    } catch (e) {
      if (alive) error = localizeError(e);
    } finally {
      if (alive) loading = false;
    }
  });

  async function attach(uri: string, name: string) {
    if (!onAttach || busy) return;
    busy = uri;
    attachError = '';
    try {
      await onAttach(uri, name);
    } catch (e) {
      if (alive) attachError = localizeError(e);
    } finally {
      if (alive) busy = null;
    }
  }
</script>

<div class="flex min-w-0 flex-col gap-2">
  {#if loading}
    <p class="text-xs text-muted-foreground" role="status">{t('conn.mcpc_res_loading')}</p>
  {:else if error}
    <p class="text-xs break-words text-destructive" role="alert">{t('conn.mcpc_res_failed', { message: error })}</p>
  {:else if list}
    {#if list.resources.length === 0 && list.templates.length === 0}
      <p class="text-xs text-muted-foreground">{t('conn.mcpc_res_none')}</p>
    {/if}
    {#if list.resources.length > 0}
      <ul class="flex flex-col gap-1" aria-label={t('conn.mcpc_res_list')}>
        {#each list.resources as r (r.uri)}
          <li class="flex min-w-0 items-center gap-2">
            <FileText class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
            <div class="flex min-w-0 flex-1 flex-col">
              <span class="truncate text-xs font-medium" title={r.description ?? r.name}>{r.title || r.name}</span>
              <code class="truncate font-mono text-[11px] text-muted-foreground" title={r.uri}>{r.uri}</code>
            </div>
            {#if r.mime_type}<Badge variant="outline" class="shrink-0 font-num text-muted-foreground">{r.mime_type}</Badge>{/if}
            {#if r.size}<span class="shrink-0 font-num text-xs text-muted-foreground">{formatBytes(r.size, 1)}</span>{/if}
            {#if onAttach}
              <Button variant="outline" size="xs" disabled={busy !== null} onclick={() => attach(r.uri, r.title || r.name)}>
                <Paperclip /> {busy === r.uri ? t('conn.mcpc_res_attaching') : t('conn.mcpc_res_attach')}
              </Button>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
    {#if list.templates.length > 0}
      <p class="mt-1 text-xs font-medium">{t('conn.mcpc_res_templates')}</p>
      <ul class="flex flex-col gap-1" aria-label={t('conn.mcpc_res_templates')}>
        {#each list.templates as tp (tp.uri_template)}
          <li class="flex min-w-0 flex-col">
            <span class="truncate text-xs" title={tp.description ?? tp.name}>{tp.title || tp.name}</span>
            <code class="truncate font-mono text-[11px] text-muted-foreground" title={tp.uri_template}>{tp.uri_template}</code>
          </li>
        {/each}
      </ul>
    {/if}
    {#if onAttach}
      <form
        class="mt-1 flex items-center gap-2"
        onsubmit={(e) => {
          e.preventDefault();
          const uri = uriInput.trim();
          if (uri) void attach(uri, '');
        }}
      >
        <Input
          class="h-8 flex-1 font-mono text-xs"
          bind:value={uriInput}
          placeholder={t('conn.mcpc_res_uri_placeholder')}
          aria-label={t('conn.mcpc_res_uri')}
          autocomplete="off"
          spellcheck="false"
        />
        <Button type="submit" variant="outline" size="sm" disabled={busy !== null || !uriInput.trim()}>
          <Paperclip /> {t('conn.mcpc_res_attach')}
        </Button>
      </form>
    {/if}
    {#if attachError}
      <p class="text-xs break-words text-destructive" role="alert">{attachError}</p>
    {/if}
  {/if}
</div>
