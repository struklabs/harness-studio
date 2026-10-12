<script lang="ts">
  // Composer button: browse the resources of the connected MCP servers and attach one to the chat
  // as a document (it is saved as a file and goes through the same document pipeline as a file
  // picked from disk). Hidden while no MCP server is configured.
  import { onDestroy, onMount } from 'svelte';
  import { Library } from '@lucide/svelte';
  import { invoke } from '$lib/ipc';
  import { Button } from '$lib/components/ui/button';
  import * as Dialog from '$lib/components/ui/dialog';
  import * as Tooltip from '$lib/components/ui/tooltip';
  import { t } from '$lib/i18n.svelte';
  import { localizeError } from '$lib/network';
  import type { McpConfig } from '$lib/mcp-types';
  import { getMcpStatus, mcpResourceToFile, type McpServerStatus } from '$lib/tools-client';
  import NativeSelect from '../dashboard/parts/NativeSelect.svelte';
  import McpResourceList from '../connections/McpResourceList.svelte';

  interface Props {
    /** Adds the saved file to the conversation's documents. */
    onAttachFile: (path: string) => Promise<void> | void;
    disabled?: boolean;
  }

  let { onAttachFile, disabled = false }: Props = $props();

  let configured = $state(false);
  let open = $state(false);
  let servers = $state<McpServerStatus[]>([]);
  let loading = $state(false);
  let error = $state('');
  let selected = $state('');
  let alive = true;

  onDestroy(() => (alive = false));
  onMount(async () => {
    try {
      const cfg = await invoke<McpConfig>('get_mcp_config');
      if (alive) configured = Object.values(cfg?.mcpServers ?? {}).some((d) => !d.disabled);
    } catch {
      // No config readable: the button stays hidden.
    }
  });

  async function openDialog() {
    open = true;
    loading = true;
    error = '';
    try {
      const all = await getMcpStatus(false);
      if (!alive) return;
      servers = all.filter((s) => s.state === 'connected' && s.resources);
      if (!servers.some((s) => s.name === selected)) selected = servers[0]?.name ?? '';
    } catch (e) {
      if (alive) error = localizeError(e);
    } finally {
      if (alive) loading = false;
    }
  }

  async function attach(uri: string, name: string) {
    const path = await mcpResourceToFile(selected, uri, name || null);
    await onAttachFile(path);
    open = false;
  }
</script>

{#if configured}
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <Button
          {...props}
          type="button"
          variant="ghost"
          size="icon-xs"
          class="mr-1"
          {disabled}
          aria-label={t('pg.mcp_res_button')}
          onclick={(e) => {
            e.currentTarget.blur();
            void openDialog();
          }}
        >
          <Library />
        </Button>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content>{t('pg.mcp_res_button')}</Tooltip.Content>
  </Tooltip.Root>

  <Dialog.Root bind:open>
    <Dialog.Content class="sm:max-w-xl">
      <Dialog.Header>
        <Dialog.Title>{t('pg.mcp_res_title')}</Dialog.Title>
        <Dialog.Description>{t('pg.mcp_res_desc')}</Dialog.Description>
      </Dialog.Header>
      <div class="flex max-h-[60vh] min-w-0 flex-col gap-3 overflow-y-auto pr-1">
        {#if loading}
          <p class="text-xs text-muted-foreground" role="status">{t('conn.mcpc_res_loading')}</p>
        {:else if error}
          <p class="text-xs break-words text-destructive" role="alert">{t('conn.mcpc_load_failed', { message: error })}</p>
        {:else if servers.length === 0}
          <p class="text-xs text-muted-foreground">{t('pg.mcp_res_none')}</p>
        {:else}
          {#if servers.length > 1}
            <NativeSelect bind:value={selected} aria-label={t('pg.mcp_res_server')}>
              {#each servers as s (s.name)}<option value={s.name}>{s.name}</option>{/each}
            </NativeSelect>
          {/if}
          {#key selected}
            <McpResourceList server={selected} onAttach={attach} />
          {/key}
        {/if}
      </div>
    </Dialog.Content>
  </Dialog.Root>
{/if}
