<script lang="ts">
  // "Update Available": a downloaded file has a newer upstream version. The flag comes from
  // `library.updates` (filled when the repo's drawer compared the file hashes), no request here.
  import { t } from '#lib/i18n.svelte.js';
  import { cn } from '#lib/utils.js';
  import { RefreshCw } from '@lucide/svelte';

  let { class: className = '' }: { class?: string } = $props();
</script>

<span class={cn('inline-flex items-center gap-1 text-warning', className)} title={t('hub.badge_update_title')}>
  <RefreshCw class="size-3" aria-hidden="true" />{t('hub.badge_update')}
</span>
