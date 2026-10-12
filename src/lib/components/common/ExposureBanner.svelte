<script lang="ts">
  import { ShieldAlert } from '@lucide/svelte';
  import { t } from '$lib/i18n.svelte';
  import { bindScope } from '$lib/network';
  import { app } from '$lib/stores/app.svelte';
  import { security } from '$lib/stores/security.svelte';

  // Persistent warning while any server runs on a network address without an API key
  // (only possible after the user's "open anyway").
  let exposed = $derived(
    app.servers.filter((s) => s.active && bindScope(s.active.bind_host) === 'lan' && !s.active.api_key_set),
  );
  let names = $derived(exposed.map((s) => app.serverLabel(s.id)).join(', '));
</script>

{#if exposed.length > 0}
  <div role="status" class="shrink-0">
    <button
      type="button"
      class="flex w-full shrink-0 items-center gap-2 border-b border-warning/40 bg-warning/10 px-4 py-1.5 text-left text-xs text-foreground transition-colors hover:bg-warning/15"
      onclick={() => security.openSettings('security')}
    >
      <ShieldAlert class="size-3.5 shrink-0 text-warning" />
      <span class="font-medium">{t('sec.banner_unprotected')}</span>
      <span class="min-w-0 truncate text-muted-foreground">{t('sec.banner_unprotected_desc', { servers: names })}</span>
    </button>
  </div>
{/if}
