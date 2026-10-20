<script lang="ts">
  import { t } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { security } from '#lib/stores/security.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  let connectors = $state(false);
  let { github = $bindable(false) }: { github?: boolean } = $props();
  function registry(kind: 'skill' | 'mcp' | 'tool') {
    app.requestedRegistryKind = kind;
    security.openSettings('tools-installed');
  }
</script>

<nav aria-label={t('discover.categories')} class="flex flex-wrap gap-2">
  <Button size="sm" variant="outline" aria-pressed={!github} onclick={() => github = false}>{t('discover.models')}</Button>
  <Button size="sm" variant="outline" aria-pressed={github} onclick={() => github = true}>{t('github.tab')}</Button>
  <Button size="sm" variant="outline" onclick={() => registry('skill')}>{t('discover.skills')}</Button>
  <Button size="sm" variant="outline" onclick={() => registry('mcp')}>{t('discover.mcp')}</Button>
  <Button size="sm" variant="outline" aria-expanded={connectors} onclick={() => connectors = !connectors}>{t('discover.connectors')}</Button>
  <Button size="sm" variant="outline" onclick={() => registry('tool')}>{t('discover.tools')}</Button>
  <Button size="sm" variant="outline" onclick={() => app.requestedTab = 'modules'}>{t('discover.modules')}</Button>
</nav>
{#if connectors}
  <div class="rounded-lg border p-3 text-xs">
    <p class="text-muted-foreground">{t('discover.connector_hint')}</p>
    <Button size="sm" variant="link" onclick={() => { app.requestedConnectionPage = 'servers'; app.requestedTab = 'connections'; }}>{t('discover.manage_mcp')}</Button>
  </div>
{/if}
