<script lang="ts">
  import HelpPopover from './common/HelpPopover.svelte';
  import { onMount } from 'svelte';
  import { app } from '#lib/stores/app.svelte.js';
  import { t } from '#lib/i18n.svelte.js';
  import SectionLoading from './common/SectionLoading.svelte';
  import ServerList from './servers/ServerList.svelte';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Plus, RefreshCw } from '@lucide/svelte';

  interface Props {
    onOpenDashboard: (id: string) => void;
    onOpenPlayground: (id: string) => void;
  }

  let { onOpenDashboard, onOpenPlayground }: Props = $props();

  let refreshing = $state(false);

  async function refresh() {
    refreshing = true;
    try {
      await Promise.all([app.refreshProfiles(), app.refreshServer()]);
    } finally {
      refreshing = false;
    }
  }

  onMount(() => {
    refresh();
  });
</script>

<!-- The page title lives in the title bar; this row only explains the list and holds its actions. -->
<div class="flex h-full w-full min-w-0 flex-col gap-4 overflow-y-auto p-4 sm:p-6">
  <div class="flex shrink-0 flex-wrap items-center justify-between gap-3">
    <div class="flex min-w-0 items-center gap-1.5"><p class="text-xs text-muted-foreground">{t('srv.intro_short')}</p><HelpPopover text={t('ui.srv_intro')} label={t('nav.servers')} /></div>
    <div class="flex min-w-0 flex-wrap items-center gap-2">
      <Button variant="outline" size="sm" onclick={refresh} disabled={refreshing}>
        <RefreshCw class={refreshing ? 'size-3.5 animate-spin' : 'size-3.5'} />
        {t('srv.refresh')}
      </Button>
      <Button size="sm" onclick={() => (app.newServerPending = true)} title={t('srv.new_server_tip')}>
        <Plus class="size-3.5" />
        {t('srv.new_server')}
      </Button>
    </div>
  </div>

  {#if refreshing}
    <SectionLoading label={t('sl.srv_label')} shape="rows" rows={3} minHeight="12rem" />
  {:else}
    <ServerList {onOpenDashboard} {onOpenPlayground} />
  {/if}

  {#if app.profiles.length > 0}
    <p class="text-xs text-muted-foreground" role="note">{t('srv.auto_start_note')}</p>
  {/if}
</div>
