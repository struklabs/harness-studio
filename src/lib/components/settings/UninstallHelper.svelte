<script lang="ts">
  // Settings > Storage: removes engines, environments, caches and logs (optionally all data),
  // then quits. The offline `harness cleanup` helper does the deletion after the app exits.
  import { invoke } from '#lib/ipc.js';
  import { t } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import SettingGroup from './SettingGroup.svelte';

  interface Plan { paths: string[]; external_models: string[] }

  let deleteData = $state(false);
  let plan = $state<Plan | null>(null);
  let external = $state<string[]>([]);
  let confirming = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function load() {
    error = null;
    confirming = false;
    external = [];
    try {
      plan = await invoke<Plan>('get_cleanup_plan', { deleteData });
    } catch (e) {
      plan = null;
      error = localizeError(e);
    }
  }

  $effect(() => {
    deleteData;
    load();
  });

  function toggle(path: string, on: boolean) {
    external = on ? [...external, path] : external.filter((p) => p !== path);
  }

  async function run() {
    busy = true;
    error = null;
    try {
      await invoke('prepare_uninstall', { deleteData, externalModels: external });
    } catch (e) {
      error = localizeError(e);
      busy = false;
    }
  }
</script>

<SettingGroup title={t('settings.reset_title')}>
  <div class="flex flex-col gap-3 px-3 py-2.5 text-sm">
    <p class="text-muted-foreground">{t('settings.reset_desc')}</p>
    <label class="flex items-center gap-2">
      <input type="checkbox" bind:checked={deleteData} disabled={busy} />
      {t('settings.reset_delete_data')}
    </label>
    {#if plan}
      <ul class="flex flex-col gap-1 font-mono text-xs text-muted-foreground">
        {#each plan.paths as path (path)}<li class="break-all">{path}</li>{/each}
      </ul>
      {#if deleteData && plan.external_models.length}
        <p class="text-xs text-muted-foreground">{t('settings.reset_external')}</p>
        {#each plan.external_models as path (path)}
          <label class="flex items-center gap-2 break-all font-mono text-xs">
            <input type="checkbox" checked={external.includes(path)} onchange={(e) => toggle(path, e.currentTarget.checked)} disabled={busy} />
            {path}
          </label>
        {/each}
      {/if}
    {/if}
    {#if error}<p class="text-xs text-destructive" role="alert">{error}</p>{/if}
    <div class="flex gap-2">
      {#if confirming}
        <Button size="sm" variant="destructive" onclick={run} disabled={busy || !plan}>{t('settings.reset_confirm')}</Button>
        <Button size="sm" variant="ghost" onclick={() => (confirming = false)} disabled={busy}>{t('settings.reset_cancel')}</Button>
      {:else}
        <Button size="sm" variant="outline" onclick={() => (confirming = true)} disabled={!plan}>{t('settings.reset_start')}</Button>
      {/if}
    </div>
  </div>
</SettingGroup>
