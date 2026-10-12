<script lang="ts">
  import { t } from '#lib/i18n.svelte.js';
  import type { GatewayStatus } from '#lib/types.js';
  import { cn } from '#lib/utils.js';
  import { checkEndpointDrift } from './integrations';

  // The gateway runs on another port than the configured one: who holds the port, and what
  // became of the client configs (how many were moved, which still need "Düzelt").
  let { status, class: className = '' }: { status: GatewayStatus; class?: string } = $props();

  let fb = $derived(status.port_fallback);
  let base = $derived(status.endpoint?.base_url ?? '');
  let moved = $derived(status.clients_moved ?? []);
  // The start's list is stale once a card was fixed: ask for the current one.
  let pending = $state<string[] | null>(null);
  let shownPending = $derived(pending ?? status.clients_pending ?? []);

  $effect(() => {
    const url = base;
    if (!url || !fb) return;
    let alive = true;
    checkEndpointDrift(url)
      .then((list) => alive && (pending = list.map((d) => d.name)))
      .catch(() => alive && (pending = null));
    return () => {
      alive = false;
    };
  });
</script>

{#if fb}
  {@const params = { configured: fb.configured, program: fb.holder?.name ?? '', port: status.port ?? '' }}
  <p class={cn('text-xs text-warning', className)} role="status">
    {fb.holder?.name ? t('gw.port_fallback', params) : t('gw.port_fallback_unknown', params)}
    {#if moved.length > 0}
      {t('gw.port_follow_moved', { count: moved.length, names: moved.join(', ') })}
    {/if}
    {#if shownPending.length > 0}
      {t('gw.port_follow_pending', { names: shownPending.join(', ') })}
    {/if}
  </p>
{/if}
