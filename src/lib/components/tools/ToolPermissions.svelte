<script lang="ts">
  import TrustPermissions from "#lib/components/tools/TrustPermissions.svelte";
  import { t } from '#lib/i18n.svelte.js';
  let { network, fs_read, fs_write }: { network: string[]; fs_read: string[]; fs_write: string[] } = $props();
</script>
<div class="rounded-md border p-3 text-xs">
  <p class="mb-2 font-medium">{t('tools.inst_confirm_asks')}</p>
  <ul class="space-y-1 break-words">
    <li>{network.includes('*') ? t('tools.network_any') : network.length ? t('tools.inst_perm_network', { hosts: network.join(', ') }) : t('tools.inst_perm_no_network')}</li>
    <li>{fs_read.length ? t('tools.inst_perm_read', { folders: fs_read.join(', ') }) : t('tools.permission_no_read')}</li>
    <li>{fs_write.length ? t('tools.inst_perm_write', { folders: fs_write.join(', ') }) : t('tools.permission_no_write')}</li>
    <li>{t('tools.inst_perm_shell')}</li>
  </ul>
</div>

<TrustPermissions kind="scoped" />
