<script lang="ts">
  import AdvancedPageNotice from './AdvancedPageNotice.svelte';
  import { PanelLeftClose, PanelLeftOpen } from '@lucide/svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { visibleGroups } from '#lib/uiLevel.js';
  import { cn } from '#lib/utils.js';
  import type { PaneState } from '#lib/stores/panes.svelte.js';
  import ResizablePane from './ResizablePane.svelte';
  import type { SectionGroup } from './sections';
  let { pane, label, groups, active, onSelect }: {
    pane: PaneState; label: string; groups: SectionGroup[]; active: string; onSelect: (id: string) => void;
  } = $props();
  let railed = $derived(!pane.expanded);
  let visible = $derived(visibleGroups(groups, uiLevel.level));
  let grouped = $derived(visible.some(group => group.label));
</script>

<ResizablePane {pane} side="left" {label} class={pane.floating ? 'bg-card' : 'bg-card/40'}>
  <div class={cn('flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto', railed ? 'px-1.5 py-3' : 'p-3')}>
    <nav class="flex flex-col gap-0.5" aria-label={label}>
      {#each visible as group, index (index)}
        {#if group.label}<h2 class={railed ? 'sr-only' : 'px-2 pt-3 pb-1 text-xs font-medium text-muted-foreground'}>{t(group.label)}</h2>{/if}
      {#each group.pages as s (s.id)}
        {@const Icon = s.icon}
        <button
          type="button"
          aria-current={active === s.id ? 'page' : undefined}
          class={cn(
            'flex min-h-8 items-center gap-2 rounded-md py-1.5 px-2 text-left text-sm leading-tight transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring/50',
            railed ? 'justify-center px-0' : grouped && 'ml-2',
            active === s.id ? 'bg-muted font-medium text-foreground' : 'text-muted-foreground hover:bg-muted/60 hover:text-foreground',
          )}
          aria-label={railed ? t(!uiLevel.advanced && s.simpleLabel ? s.simpleLabel : s.label) : undefined}
          title={t(!uiLevel.advanced && s.simpleLabel ? s.simpleLabel : s.label)}
          data-section={s.id}
          onclick={() => onSelect(s.id)}
        >
          <Icon class="size-4 shrink-0" />
          {#if !railed}<span class="min-w-0 break-words">{t(!uiLevel.advanced && s.simpleLabel ? s.simpleLabel : s.label)}</span>{/if}
        </button>
      {/each}
      {/each}
    </nav>
    {#if !railed && visible.flatMap(g => g.pages).length < groups.flatMap(g => g.pages).length}<AdvancedPageNotice discoverable />{/if}
  </div>
  <div class={cn('shrink-0 border-t border-border py-2', railed ? 'px-1.5' : 'px-3')}>
    <button
      type="button"
      class={cn(
        'flex h-8 w-full items-center gap-2 rounded-md px-2 text-sm text-muted-foreground transition-colors outline-none hover:bg-muted/60 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50',
        railed && 'justify-center px-0'
      )}
      aria-controls={pane.elementId}
      aria-expanded={!railed}
      aria-label={railed ? t('pane.show', { name: label }) : t('pane.hide', { name: label })}
      title={railed ? t('pane.show', { name: label }) : t('pane.hide', { name: label })}
      onclick={() => pane.toggle()}
    >
      {#if railed}
        <PanelLeftOpen class="size-4" />
      {:else}
        <PanelLeftClose class="size-4" />
        <span class="truncate">{t('sidebar.collapse')}</span>
      {/if}
    </button>
  </div>
  </ResizablePane>
