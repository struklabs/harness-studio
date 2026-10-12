<script lang="ts">
  import type { LocalModel } from '$lib/types';
  import { t } from '$lib/i18n.svelte';
  import { sourceLabel, type LocalModelExt } from '$lib/import-types';
  import { Badge } from '$lib/components/ui/badge';

  interface Props {
    models: LocalModel[];
    modelsDir: string;
  }

  let { models, modelsDir }: Props = $props();

  // Only what llama-server can serve: chat, embedding and reranker models (not whisper, image or speech).
  let ready = $derived(
    models.filter((m) => m.is_ready && (!m.kind || m.kind === 'llm' || m.kind === 'embedding' || m.kind === 'reranker'))
  );
</script>

<div class="flex flex-col gap-2 rounded-md border border-border bg-muted/30 p-2.5">
  <div class="flex items-center justify-between gap-2">
    <span class="text-xs font-medium">{t('dash.ms_router_banner_title')}</span>
    <Badge variant="outline" class="text-muted-foreground">{t('dash.ms_router_badge')}</Badge>
  </div>
  <p class="text-xs leading-snug text-muted-foreground">
    <code class="rounded bg-muted px-1 font-mono text-foreground">{modelsDir}</code>
    {t('dash.ms_router_desc_1')} <code class="font-mono">/v1/models</code> {t('dash.ms_router_desc_2')}
    <code class="font-mono">/v1/chat/completions</code> {t('dash.ms_router_desc_3')}
  </p>
  <div class="flex flex-col gap-1.5">
    <span class="text-xs font-medium text-muted-foreground">{t('dash.ms_router_list_header', { count: ready.length })}</span>
    <div class="flex max-h-24 flex-wrap gap-1 overflow-y-auto">
      {#each ready as m (m.id)}
        {@const src = (m as LocalModelExt).source ?? null}
        <Badge variant="outline" class="max-w-full font-normal" title={m.path}>
          <span class="truncate font-medium">{m.name}</span>
          <span class="font-mono text-xs text-muted-foreground">{m.quantization}{src ? ` · ${sourceLabel(src)}` : ''}</span>
        </Badge>
      {/each}
    </div>
  </div>
</div>
