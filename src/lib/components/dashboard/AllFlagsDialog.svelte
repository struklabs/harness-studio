<script lang="ts">
  import HelpPopover from '$lib/components/common/HelpPopover.svelte';
  import FieldHelp from '$lib/components/common/FieldHelp.svelte';
  import { invoke } from '$lib/ipc';
  import { untrack } from 'svelte';
  import type { RuntimeFlag } from '$lib/types';
  import { t } from '$lib/i18n.svelte';
  import { capsStore } from '$lib/stores/caps.svelte';
  import * as Dialog from '$lib/components/ui/dialog';
  import * as Tooltip from '$lib/components/ui/tooltip';
  import { Input } from '$lib/components/ui/input';
  import { Button } from '$lib/components/ui/button';
  import { Badge } from '$lib/components/ui/badge';
  import { Plus } from '@lucide/svelte';

  interface Props {
    serverId: string;
    /** Appends the flag to the custom arguments. */
    onAdd: (flag: string) => void;
    onClose: () => void;
  }

  let { serverId, onAdd, onClose }: Props = $props();

  const uid = $props.id();
  let flags = $state<RuntimeFlag[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let query = $state('');

  $effect(() => {
    const id = serverId;
    loading = true;
    invoke<RuntimeFlag[]>('list_runtime_flags', { serverId: id, runtimeId: untrack(() => capsStore.runtimeId) })
      .then((list) => {
        flags = list.sort((a, b) => a.canonical.localeCompare(b.canonical));
        error = null;
      })
      .catch((e) => (error = String(e)))
      .finally(() => (loading = false));
  });

  let filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return flags;
    return flags.filter(
      (f) =>
        f.aliases.some((a) => a.toLowerCase().includes(q)) ||
        f.description.toLowerCase().includes(q) ||
        (f.env ?? '').toLowerCase().includes(q)
    );
  });

  let build = $derived(
    capsStore.caps?.version.build != null ? `b${capsStore.caps.version.build}` : (capsStore.caps?.version.raw ?? '?')
  );

  /** Only flags that aren't settings already, aren't managed by us and still exist. */
  function canAdd(f: RuntimeFlag): boolean {
    return !f.protected && !f.param_id && !f.removed;
  }
</script>

<Dialog.Root open={true} onOpenChange={(open) => !open && onClose()}>
  <Dialog.Content class="flex max-h-[85vh] flex-col sm:max-w-2xl">
    <Dialog.Header>
      <Dialog.Title>{t('caps.flags_title')}</Dialog.Title>
      <Dialog.Description>{t('caps.flags_desc', { build })}</Dialog.Description>
    </Dialog.Header>

    <div class="flex items-center gap-2">
      <FieldHelp text={t('help.flags_search')}><Input id="{uid}-q" type="search" placeholder={t('caps.flags_search')} bind:value={query} class="flex-1" aria-label={t('caps.flags_search')} /></FieldHelp>
      <span class="shrink-0 font-num text-xs text-muted-foreground">
        {t('caps.flags_count', { shown: filtered.length, total: flags.length })}
      </span>
    </div>

    <div class="-mx-1 min-h-0 flex-1 overflow-y-auto px-1" role="list">
      {#if loading}
        <p class="py-6 text-center text-xs text-muted-foreground">{t('caps.flags_loading')}</p>
      {:else if error}
        <p class="rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-2 text-xs text-destructive" role="alert">
          {t('caps.flags_load_error', { error })}
        </p>
      {:else if filtered.length === 0}
        <p class="py-6 text-center text-xs text-muted-foreground">{t('caps.flags_empty')}</p>
      {:else}
        {#each filtered as f (f.canonical)}
          <div class="flex items-start gap-3 border-b border-border py-2 last:border-b-0" role="listitem">
            <div class="flex min-w-0 flex-1 flex-col gap-1">
              <div class="flex flex-wrap items-center gap-1.5">
                <code class="font-mono text-xs font-semibold text-muted-foreground">{f.aliases.join(', ')}</code>
                {#if f.description}<HelpPopover text={f.description} label={f.canonical} />{/if}
                {#if f.value_hint}<code class="font-mono text-xs text-muted-foreground">{f.value_hint}</code>{/if}
                {#if f.param_id}<Badge variant="secondary" class="h-4 px-1.5 text-xs">{t('caps.flags_in_catalog')}</Badge>{/if}
                {#if f.protected}
                  <Tooltip.Root>
                    <Tooltip.Trigger class="inline-flex">
                      <Badge variant="outline" class="h-4 px-1.5 text-xs text-warning">{t('caps.flags_protected')}</Badge>
                    </Tooltip.Trigger>
                    <Tooltip.Content class="max-w-64">{t('caps.flags_protected_tip')}</Tooltip.Content>
                  </Tooltip.Root>
                {/if}
                {#if f.removed}<Badge variant="destructive" class="h-4 px-1.5 text-xs">{t('caps.flags_removed')}</Badge>{/if}
                {#if f.deprecated}<Badge variant="outline" class="h-4 px-1.5 text-xs">{t('caps.flags_deprecated')}</Badge>{/if}
              </div>
              <div class="flex flex-wrap gap-x-3 gap-y-0.5 text-xs text-muted-foreground">
                {#if f.enum_values.length}<span>{t('caps.flags_values')}: <code class="font-mono">{f.enum_values.join(' | ')}</code></span>{/if}
                {#if f.default}<span>{t('caps.flags_default')}: <code class="font-mono">{f.default}</code></span>{/if}
                {#if f.env}<span>{t('caps.flags_env')}: <code class="font-mono">{f.env}</code></span>{/if}
              </div>
            </div>
            <Button
              variant="outline"
              size="xs"
              class="shrink-0"
              disabled={!canAdd(f)}
              onclick={() => onAdd(f.canonical)}
              aria-label="{t('caps.flags_add')} {f.canonical}"
            >
              <Plus class="size-3" />
              {t('caps.flags_add')}
            </Button>
          </div>
        {/each}
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>
