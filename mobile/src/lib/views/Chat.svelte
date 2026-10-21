<script lang="ts">
  import { marked } from 'marked';
  import DOMPurify from 'dompurify';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import { t } from '#lib/i18n.svelte.js';
  import { chat, parseTargetKey, targetKey } from '../chat.svelte.ts';
  import { connections } from '../connections.svelte.ts';
  import { gatewayModels, type GatewayModel } from '../harness.ts';
  import { providerLabel } from '../providers.ts';
  import { errorText } from '../text.ts';
  import Placement from './Placement.svelte';

  let { onConnect }: { onConnect: () => void } = $props();

  let harnessModels = $state<Record<string, GatewayModel[]>>({});
  let draft = $state('');
  let list: HTMLElement | undefined = $state();

  $effect(() => {
    for (const c of connections.harness) {
      if (!(c.id in harnessModels)) {
        harnessModels[c.id] = [];
        gatewayModels(c).then((m) => (harnessModels[c.id] = m)).catch(() => {});
      }
    }
    for (const p of connections.providers) if (!connections.status[p.id]) void connections.refreshProvider(p);
  });

  // Follow the reply while it streams.
  $effect(() => {
    void chat.turns.at(-1)?.content;
    list?.scrollTo({ top: list.scrollHeight });
  });

  const selected = $derived(chat.target ? targetKey(chat.target) : '');
  const empty = $derived(!connections.harness.length && !connections.providers.length);

  function pick(e: Event) {
    chat.target = parseTargetKey((e.currentTarget as HTMLSelectElement).value);
  }

  function render(markdown: string): string {
    return DOMPurify.sanitize(marked.parse(markdown, { async: false }) as string, { FORBID_TAGS: ['img', 'style', 'form', 'input'] });
  }

  function send(e: SubmitEvent) {
    e.preventDefault();
    const text = draft;
    draft = '';
    void chat.send(text);
  }
</script>

{#if empty}
  <div class="space-y-3 p-6 text-center">
    <p class="text-sm text-muted-foreground">{t('mobile.chat.no_connections')}</p>
    <Button onclick={onConnect}>{t('mobile.conn.add_first')}</Button>
  </div>
{:else}
  <div class="flex h-full flex-col">
    <div class="flex items-center gap-2 border-b border-border p-2">
      <select aria-label={t('mobile.chat.model')} class="h-9 min-w-0 flex-1 rounded-md border border-input bg-background px-2 text-sm" value={selected} onchange={pick} data-testid="model-picker">
        <option value="" disabled>{t('mobile.chat.pick_model')}</option>
        {#each connections.harness as c (c.id)}
          <optgroup label={t('mobile.chat.group_harness', { name: c.name })}>
            {#each harnessModels[c.id] ?? [] as m (m.id)}<option value={targetKey({ type: 'harness', connection: c.id, model: m.id })}>{m.id}</option>{/each}
          </optgroup>
        {/each}
        {#each connections.providers as p (p.id)}
          <optgroup label={t('mobile.chat.group_provider', { name: p.kind === 'custom' ? p.name : providerLabel(p.kind) })}>
            {#each connections.status[p.id]?.models ?? [] as m (m.id)}<option value={targetKey({ type: 'provider', connection: p.id, model: m.id })}>{m.displayName ?? m.id}</option>{/each}
          </optgroup>
        {/each}
      </select>
      <Button variant="ghost" size="sm" onclick={() => chat.clear()} disabled={!chat.turns.length}>{t('mobile.chat.new')}</Button>
    </div>

    <div bind:this={list} class="min-h-0 flex-1 space-y-3 overflow-y-auto p-3">
      {#each chat.turns as turn, i (i)}
        {#if turn.role === 'user'}
          <div class="ml-10 rounded-2xl rounded-br-sm bg-primary px-3 py-2 text-sm whitespace-pre-wrap text-primary-foreground">{turn.content}</div>
        {:else}
          <div class="mr-4" data-testid="assistant-turn">
            <div class="prose-sm rounded-2xl rounded-bl-sm bg-muted px-3 py-2 text-sm break-words" data-testid="assistant-text">
              {#if turn.content}{@html render(turn.content)}{:else if turn.streaming}<span class="text-muted-foreground">…</span>{/if}
            </div>
            {#if turn.error}<p class="mt-1 text-xs text-destructive" role="alert">{errorText(turn.error)}{turn.detail ? ` — ${turn.detail}` : ''}</p>{/if}
            {#if turn.placement}<Placement placement={turn.placement} model={chat.target?.type === 'provider' ? chat.providerModel() : null} usage={turn.usage} cost={turn.cost ?? null} />{/if}
          </div>
        {/if}
      {/each}
    </div>

    <form class="flex items-end gap-2 border-t border-border p-2" onsubmit={send}>
      <Textarea bind:value={draft} rows={1} class="max-h-32 min-h-9 flex-1" placeholder={t('mobile.chat.placeholder')} aria-label={t('mobile.chat.placeholder')} />
      {#if chat.busy}
        <Button type="button" variant="outline" onclick={() => chat.stop()}>{t('mobile.chat.stop')}</Button>
      {:else}
        <Button type="submit" disabled={!chat.target || !draft.trim()} data-testid="send">{t('mobile.chat.send')}</Button>
      {/if}
    </form>
  </div>
{/if}
