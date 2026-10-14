<script lang="ts">
  // The models of a running router server (one llama-server process, many models): each shows
  // whether it is loaded and can be loaded or unloaded without touching the others.
  import { invoke } from '#lib/ipc.js';
  import type { RouterModel, RouterModelState } from '#lib/types.js';
  import { t } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { AlertCircle, Play, Square } from '@lucide/svelte';
  import { Button } from '#lib/components/ui/button/index.js';
  import { cn } from '#lib/utils.js';

  interface Props {
    serverId: string;
  }

  let { serverId }: Props = $props();

  let models = $state<RouterModel[]>([]);
  let loaded = $state(false);
  let error = $state('');
  let busy = $state<Record<string, boolean>>({});

  const stateKey = {
    loaded: 'router.state_loaded',
    loading: 'router.state_loading',
    unloaded: 'router.state_unloaded',
    failed: 'router.state_failed',
  } as const satisfies Record<RouterModelState, string>;

  async function refresh() {
    try {
      models = await invoke<RouterModel[]>('list_router_models', { serverId });
      error = '';
    } catch (e) {
      error = localizeError(e);
    } finally {
      loaded = true;
    }
  }

  // Polls while the list is shown; faster while a model is loading.
  $effect(() => {
    void serverId;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => {
      await refresh();
      if (!stopped) timer = setTimeout(tick, models.some((m) => m.state === 'loading') ? 1000 : 3000);
    };
    tick();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  });

  async function act(model: RouterModel, command: 'load_router_model' | 'unload_router_model') {
    busy[model.id] = true;
    try {
      await invoke<RouterModel>(command, { serverId, model: model.id });
      error = '';
    } catch (e) {
      error = localizeError(e);
    } finally {
      busy[model.id] = false;
      await refresh();
    }
  }
</script>

<div class="flex flex-col gap-2 py-1" data-testid="router-models">
  <div class="flex flex-col gap-0.5">
    <h3 class="text-xs font-semibold text-foreground">{t('router.models_title')}</h3>
    <p class="text-xs text-muted-foreground">{t('router.models_hint')}</p>
  </div>
  {#if error}
    <span class="flex items-center gap-1 text-xs text-destructive" role="alert">
      <AlertCircle class="size-3 shrink-0" />
      <span class="truncate" title={error}>{error}</span>
    </span>
  {/if}
  {#if loaded && models.length === 0 && !error}
    <p class="text-xs text-muted-foreground">{t('router.no_models')}</p>
  {:else if models.length > 0}
    <ul class="flex flex-col divide-y divide-border rounded-md border border-border">
      {#each models as m (m.id)}
        <li class="flex items-center gap-3 px-3 py-1.5">
          <span class="min-w-0 flex-1 truncate font-mono text-xs text-foreground" title={m.path ?? m.id}>{m.id}</span>
          <span
            class={cn(
              'shrink-0 text-xs',
              m.state === 'loaded' && 'text-foreground',
              m.state === 'failed' ? 'text-destructive' : m.state !== 'loaded' && 'text-muted-foreground'
            )}
            role="status"
          >
            {t(stateKey[m.state])}
          </span>
          {#if m.state === 'loaded' || m.state === 'loading'}
            <Button
              variant="outline"
              size="icon-sm"
              title={t('router.unload')}
              aria-label={t('router.unload')}
              disabled={busy[m.id] || m.state === 'loading'}
              onclick={() => act(m, 'unload_router_model')}
            >
              <Square class="size-3.5" />
            </Button>
          {:else}
            <Button
              variant="outline"
              size="icon-sm"
              title={t('router.load')}
              aria-label={t('router.load')}
              disabled={busy[m.id]}
              onclick={() => act(m, 'load_router_model')}
            >
              <Play class="size-3.5" />
            </Button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>
