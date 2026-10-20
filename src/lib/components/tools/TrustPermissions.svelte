<script lang="ts">
  import { t } from '#lib/i18n.svelte.js';
  let { kind }: { kind: 'native' | 'remote' | 'scoped' | 'agent' | 'trainer' } = $props();
</script>
<div class="space-y-1 rounded-md border p-3 text-xs">
  <p class="font-medium">{t('trust.title')}: {t(kind === 'agent' ? 'trust.native' : kind === 'trainer' ? 'trust.trainer' : kind === 'remote' ? 'trust.remote' : kind === 'scoped' ? 'trust.scoped' : 'trust.native')}</p>
  {#if kind === 'native' || kind === 'agent'}
    <p>{t('trust.native_files')}</p><p>{t('trust.native_network')}</p><p>{t('trust.native_accounts')}</p>
  {:else if kind === 'remote'}
    <p>{t('trust.remote_access')}</p><p>{t('trust.remote_data')}</p>
  {:else if kind === 'scoped'}
    <p>{t('trust.scoped_access')}</p><p>{t('trust.origin')}</p>
  {:else}<p>{t('trust.future')}</p>{/if}
  {#if kind !== 'trainer'}<p class="text-muted-foreground">{t(kind === 'agent' ? 'trust.revoke_agent' : kind === 'scoped' ? 'trust.revoke_tools' : 'trust.revoke_native')}</p>{/if}
</div>
