<script lang="ts">
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { training } from '#lib/stores/training.svelte.js';
  import type { SupportLevel } from '#lib/training-types.js';

  let compat = $derived(training.compat);

  const CHIP: Record<SupportLevel, string> = {
    supported: 'bg-emerald-500/15 text-emerald-600 dark:text-emerald-400',
    limited: 'bg-amber-500/15 text-amber-700 dark:text-amber-400',
    unsupported: 'bg-muted text-muted-foreground',
  };
</script>

<section class="rounded-lg border border-border p-4" aria-label={t('training.compat_title')}>
  <h3 class="text-sm font-semibold">{t('training.compat_title')}</h3>
  {#if compat}
    <p class="mt-1 text-xs text-muted-foreground">
      {t('training.compat_machine', { os: compat.os, devices: compat.devices.map((d) => t(`training.device_${d}` as TKey)).join(', ') })}
      {#if !compat.probed}· {t('training.compat_unprobed')}{/if}
    </p>
    <table class="mt-3 w-full text-sm">
      <thead class="text-left text-xs text-muted-foreground">
        <tr>
          <th scope="col" class="w-40 pb-1 font-medium">{t('training.compat_method')}</th>
          <th scope="col" class="w-28 pb-1 font-medium">{t('training.compat_status')}</th>
          <th scope="col" class="pb-1 font-medium">{t('training.compat_reason')}</th>
        </tr>
      </thead>
      <tbody>
        {#each compat.methods as m (m.method)}
          <tr class="border-t border-border align-top">
            <th scope="row" class="py-2 pr-2 text-left font-medium">{t(`training.method_name_${m.method}` as TKey)}</th>
            <td class="py-2 pr-2"><span class="rounded-full px-2 py-0.5 text-xs font-medium {CHIP[m.support]}">{t(`training.support_${m.support}` as TKey)}</span></td>
            <td class="py-2 text-xs text-muted-foreground">{t(m.reason as TKey, m.params)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="mt-2 text-xs text-muted-foreground">{t('training.compat_loading')}</p>
  {/if}
</section>
