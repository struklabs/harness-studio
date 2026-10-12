<script lang="ts">
  import { app } from '$lib/stores/app.svelte';
  import { t } from '$lib/i18n.svelte';
  import { Button } from '$lib/components/ui/button';
  import type { LaunchParamsForm } from '../params';
  import AdvSection from './AdvSection.svelte';
  import SwitchRow from './SwitchRow.svelte';
  import { capsStore } from '$lib/stores/caps.svelte';

  interface Props {
    params: LaunchParamsForm;
    onGoToConnections: () => void;
  }

  let { params = $bindable(), onGoToConnections }: Props = $props();

  const uid = $props.id();
</script>

<!-- Harness's own tools (Settings > Tools) run in Harness; this section is only llama-server's
     own MCP option. -->
<AdvSection open={true} title={t('dash.n_sec_tools')}>
  {#if capsStore.supports('mcp_enabled') || params.mcp_enabled}
  <div class="flex flex-col gap-1.5">
    <SwitchRow id="{uid}-mcp" feature="mcp" label={t('dash.adv_mcp_label')} hint={t('dash.adv_mcp_hint')} note={t('dash.mcp_note')} bind:checked={params.mcp_enabled} />
    <Button variant="link" size="xs" class="h-auto self-start px-0 text-xs" onclick={() => { app.requestedConnectionPage = 'servers'; onGoToConnections(); }}>
      {t('dash.adv_mcp_go_connections')}
    </Button>
  </div>
  {/if}
</AdvSection>
