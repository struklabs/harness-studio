<script lang="ts">
  // Settings > Storage: opens the uninstall window (choose what to remove). The app quits first;
  // the window is a separate small process, so no database or engine is open while it deletes.
  import { invoke } from '#lib/ipc.js';
  import { t } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import SettingGroup from './SettingGroup.svelte';
  import SettingRow from './SettingRow.svelte';

  let busy = $state(false);
  let error = $state<string | null>(null);

  async function open() {
    busy = true;
    error = null;
    try {
      await invoke('launch_uninstaller');
    } catch (e) {
      error = localizeError(e);
      busy = false;
    }
  }
</script>

<SettingGroup title={t('settings.uninstall_title')} danger>
  <SettingRow label={t('settings.uninstall_title')} hint={t('settings.uninstall_desc')}>
    <Button size="sm" variant="destructive" onclick={open} disabled={busy}>{t('settings.uninstall_open')}</Button>
  </SettingRow>
  {#if error}<p class="px-3 pb-2 text-xs text-destructive" role="alert">{error}</p>{/if}
</SettingGroup>
