<script lang="ts">
  import { untrack } from 'svelte';
  import { emit } from '@tauri-apps/api/event';
  import { invoke } from '#lib/ipc.js';
  import { openPath } from '@tauri-apps/plugin-opener';
  import { t } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Switch } from '#lib/components/ui/switch/index.js';
  import SettingGroup from '../settings/SettingGroup.svelte';
  export interface Skill { name: string; description: string; enabled: boolean; error: string | null }
  let { revision = 0, onChanged = () => {} }: { revision?: number; onChanged?: () => void } = $props();
  let items = $state<Skill[]>([]);
  let error = $state('');
  let busy = $state(false);
  let removing = $state<string | null>(null);
  async function load() { items = await invoke<Skill[]>('skills_list'); }
  async function change(command: string, args: Record<string, unknown>) {
    await invoke(command, args); await emit('tools-changed'); onChanged();
  }
  async function act(work: () => Promise<void>) {
    if (busy) return;
    busy = true; error = '';
    try { await work(); await load(); } catch (e) { error = localizeError(e); } finally { busy = false; }
  }
  $effect(() => { void revision; untrack(() => void act(load)); });
</script>

<SettingGroup title={t('tools.skills_title')} hint={t('tools.skills_desc')}>
  <div class="flex flex-col gap-3 p-4">
    <div class="flex flex-wrap gap-2">
      <Button variant="outline" size="sm" disabled={busy} onclick={() => act(async () => { await openPath(await invoke<string>('skills_folder')); })}>{t('tools.skills_folder')}</Button>
      <Button variant="outline" size="sm" disabled={busy} onclick={() => act(async () => { await emit('tools-changed'); onChanged(); })}>{t('create.refresh')}</Button>
    </div>
    {#if error}<p role="alert" class="text-xs text-destructive">{error}</p>{/if}
    {#each items as skill (skill.name)}
      <article class="flex flex-col gap-2 rounded-md border p-3">
        <div class="flex items-center justify-between gap-3"><h3 class="font-mono text-sm">{skill.name}</h3><Switch checked={skill.enabled} disabled={busy || !!skill.error} aria-label={skill.name} onCheckedChange={enabled => act(() => change('skills_set_enabled', { name: skill.name, enabled }))} /></div>
        <p class="text-xs text-muted-foreground">{skill.description}</p>
        {#if skill.error}<p role="alert" class="text-xs text-destructive">{t('tools.skills_invalid', { detail: skill.error })}</p>{/if}
        {#if removing === skill.name}
          <p class="text-xs">{t('tools.skills_remove_confirm')}</p>
          <div class="flex gap-2"><Button size="sm" variant="outline" disabled={busy} onclick={() => removing = null}>{t('tools.inst_cancel')}</Button><Button size="sm" disabled={busy} onclick={() => act(async () => { await change('skills_remove', { name: skill.name }); removing = null; })}>{t('tools.inst_remove')}</Button></div>
        {:else}<Button variant="ghost" size="sm" disabled={busy} onclick={() => removing = skill.name}>{t('tools.inst_remove')}</Button>{/if}
      </article>
    {:else}<p class="text-xs text-muted-foreground">{t('tools.skills_empty')}</p>{/each}
  </div>
</SettingGroup>
