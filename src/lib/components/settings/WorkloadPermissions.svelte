<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { errorText } from '#lib/errors.js';
  import { locale, t, type TKey } from '#lib/i18n.svelte.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import SettingGroup from './SettingGroup.svelte';

  type Kind = 'heavy_gpu' | 'heavy_cpu' | 'microphone' | 'camera';
  interface Consent { id: string; client: string; kind: Kind; choice: 'allow' | 'deny'; decided_at_ms: number }
  interface View { kind: Kind; os: 'allowed' | 'denied' | 'unknown' | null; consents: Consent[] }

  let views = $state<View[]>([]);
  let error = $state('');
  let pending = $state(false);

  async function load() {
    try { views = await invoke<View[]>('workloads_list'); }
    catch (e) { error = errorText(e); }
  }
  onMount(() => { void load(); });

  async function revoke(c: Consent) {
    pending = true; error = '';
    try { await invoke('workloads_revoke', { consentId: c.id }); await load(); }
    catch (e) { error = errorText(e); }
    finally { pending = false; }
  }
  const clientLabel = (c: Consent) => (c.client === 'ui' ? t('wl.client_ui') : c.client);
  const since = (c: Consent) => t('wl.since', { date: new Date(c.decided_at_ms).toLocaleString(locale()) });
</script>

<div class="flex flex-col gap-3" data-testid="workload-permissions">
  <div class="flex flex-col gap-1">
    <h3 class="text-sm font-semibold">{t('wl.title')}</h3>
    <p class="text-xs text-muted-foreground">{t('wl.intro')}</p>
    <p class="text-xs text-muted-foreground">{t('wl.desktop_note')}</p>
  </div>
  {#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
  {#each views as v (v.kind)}
    <SettingGroup title={t(('wl.kind_' + v.kind) as TKey)}>
      <div class="flex flex-col gap-2 px-4 py-3" data-workload={v.kind}>
        <p class="text-xs text-muted-foreground">{t(('wl.help_' + v.kind) as TKey)}</p>
        {#if v.os}
          <p class="text-xs {v.os === 'denied' ? 'text-destructive' : 'text-muted-foreground'}">{t(('wl.os_' + v.os) as TKey)}</p>
        {/if}
        {#each v.consents as c (c.id)}
          <div class="flex flex-wrap items-center gap-2 rounded border px-2 py-1 text-xs" data-consent={c.id}>
            <span class="break-all font-medium">{clientLabel(c)}</span>
            <Badge variant={c.choice === 'allow' ? 'secondary' : 'outline'}>{t(c.choice === 'allow' ? 'wl.allowed' : 'wl.denied')}</Badge>
            <span class="text-muted-foreground">{since(c)}</span>
            <span class="flex-1"></span>
            <Button variant="outline" size="sm" disabled={pending} onclick={() => revoke(c)}>{t('wl.revoke')}</Button>
          </div>
        {:else}
          <p class="text-xs text-muted-foreground">{t('wl.none')}</p>
        {/each}
      </div>
    </SettingGroup>
  {/each}
</div>
