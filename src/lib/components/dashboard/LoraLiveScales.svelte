<script lang="ts">
  import { fixed } from '#lib/format.js';
  import { invoke } from '#lib/ipc.js';
  import type { LoraAdapterLive, ServerStatus } from '#lib/types.js';
  import { t } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Slider } from '#lib/components/ui/slider/index.js';
  import { toast } from 'svelte-sonner';

  interface Props {
    serverStatus: ServerStatus;
    serverId: string;
  }

  let { serverStatus, serverId }: Props = $props();

  const MIN = 0;
  const MAX = 2;
  const STEP = 0.05;

  let adapters = $state<LoraAdapterLive[]>([]);
  /** Slider positions by adapter id (what Apply sends). */
  let draft = $state<Record<number, number>>({});
  let busy = $state(false);

  let running = $derived(serverStatus === 'Running');
  let dirty = $derived(adapters.some((a) => !same(draft[a.id], a.scale)));

  const same = (a: number | undefined, b: number) => a !== undefined && Math.abs(a - b) < 1e-9;
  const round = (n: number) => Math.round(n / STEP) * STEP;
  const fileName = (p: string) => p.split(/[\/]/).pop() || p;

  function take(list: LoraAdapterLive[]) {
    adapters = list;
    draft = Object.fromEntries(list.map((a) => [a.id, a.scale]));
  }

  // The list is read when the server is up and again for another server; a server without
  // adapters (or without the route) simply shows nothing.
  $effect(() => {
    const id = serverId;
    if (!running) {
      adapters = [];
      draft = {};
      return;
    }
    let cancelled = false;
    invoke<LoraAdapterLive[]>('list_lora_adapters', { serverId: id })
      .then((list) => {
        if (!cancelled) take(list);
      })
      .catch(() => {
        if (!cancelled) take([]);
      });
    return () => {
      cancelled = true;
    };
  });

  async function apply() {
    busy = true;
    try {
      const scales = adapters.filter((a) => !same(draft[a.id], a.scale)).map((a) => ({ id: a.id, scale: draft[a.id] }));
      take(await invoke<LoraAdapterLive[]>('set_lora_scales', { serverId, scales }));
      toast.success(t('dash.lora_live_applied'));
    } catch (e) {
      toast.error(localizeError(e));
    } finally {
      busy = false;
    }
  }
</script>

{#if running && adapters.length > 0}
  <section class="flex flex-col gap-2 border-b border-border px-4 py-3 text-xs" aria-label={t('dash.lora_live_title')}>
    <div class="flex items-baseline justify-between gap-3">
      <span class="font-medium text-foreground" title={t('dash.lora_live_hint')}>{t('dash.lora_live_title')}</span>
      <Button type="button" size="xs" disabled={!dirty || busy} onclick={apply}>{t('dash.lora_live_apply')}</Button>
    </div>
    {#each adapters as a (a.id)}
      <div class="flex items-center gap-3">
        <span class="w-40 shrink-0 truncate font-mono text-muted-foreground" title={a.path}>{a.id} · {fileName(a.path)}</span>
        <Slider
          type="single"
          min={MIN}
          max={MAX}
          step={STEP}
          value={draft[a.id] ?? a.scale}
          onValueChange={(v) => (draft[a.id] = round(v))}
          aria-label={t('dash.lora_live_scale_aria', { name: fileName(a.path) })}
          class="flex-1"
          disabled={busy}
        />
        <span class="w-10 shrink-0 text-right font-num">{fixed(draft[a.id] ?? a.scale, 2)}</span>
      </div>
    {/each}
  </section>
{/if}
