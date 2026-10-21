<script lang="ts">
  import MessageSquare from '@lucide/svelte/icons/message-square';
  import ListChecks from '@lucide/svelte/icons/list-checks';
  import SlidersHorizontal from '@lucide/svelte/icons/sliders-horizontal';
  import FileText from '@lucide/svelte/icons/file-text';
  import Plug from '@lucide/svelte/icons/plug';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { connections } from '../lib/connections.svelte.ts';
  import Chat from '../lib/views/Chat.svelte';
  import Jobs from '../lib/views/Jobs.svelte';
  import Manage from '../lib/views/Manage.svelte';
  import Files from '../lib/views/Files.svelte';
  import Connections from '../lib/views/Connections.svelte';
  import { runE2e } from '../lib/e2e.ts';
  import { onMount } from 'svelte';

  type Tab = 'chat' | 'jobs' | 'manage' | 'files' | 'connections';
  const tabs: { id: Tab; label: TKey; icon: typeof Plug }[] = [
    { id: 'chat', label: 'mobile.tab.chat', icon: MessageSquare },
    { id: 'jobs', label: 'mobile.tab.jobs', icon: ListChecks },
    { id: 'manage', label: 'mobile.tab.manage', icon: SlidersHorizontal },
    { id: 'files', label: 'mobile.tab.files', icon: FileText },
    { id: 'connections', label: 'mobile.tab.connections', icon: Plug },
  ];
  let tab = $state<Tab>(connections.harness.length || connections.providers.length ? 'chat' : 'connections');
  onMount(() => void runE2e((next) => (tab = next)));
</script>

<div class="flex h-full flex-col">
  <header class="safe-top border-b border-border bg-card px-4 pb-2">
    <h1 class="pt-2 text-base font-semibold">{t(tabs.find((x) => x.id === tab)!.label)}</h1>
  </header>
  <main class="min-h-0 flex-1 overflow-y-auto" data-testid="tab-{tab}">
    {#if tab === 'chat'}
      <Chat onConnect={() => (tab = 'connections')} />
    {:else if tab === 'jobs'}
      <Jobs />
    {:else if tab === 'manage'}
      <Manage />
    {:else if tab === 'files'}
      <Files />
    {:else}
      <Connections />
    {/if}
  </main>
  <nav class="safe-bottom grid grid-cols-5 border-t border-border bg-card" aria-label={t('mobile.nav')}>
    {#each tabs as item (item.id)}
      {@const Icon = item.icon}
      <button
        type="button"
        class="flex flex-col items-center gap-0.5 py-2 text-[11px] {tab === item.id ? 'text-primary' : 'text-muted-foreground'}"
        aria-current={tab === item.id ? 'page' : undefined}
        onclick={() => (tab = item.id)}
      >
        <Icon class="size-5" />
        {t(item.label)}
      </button>
    {/each}
  </nav>
</div>
