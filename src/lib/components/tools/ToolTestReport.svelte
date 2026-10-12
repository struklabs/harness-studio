<script lang="ts">
  // The result of a package's test (installed tools in Settings, a draft in the generator
  // dialog): one line per case, the reason a failed case failed in words.
  import { Check, X } from '@lucide/svelte';
  import { t, type TKey } from '$lib/i18n.svelte';
  import { cn } from '$lib/utils';
  import type { TestReport } from '$lib/tools-client';
  import { localizeError } from '$lib/network';

  let { report }: { report: TestReport } = $props();

  const passed = $derived(report.cases.filter((c) => c.passed).length);
  const clip = (s: string) => (s.length > 200 ? `${s.slice(0, 200)}…` : s);
</script>

<div class="flex flex-col gap-1.5 text-xs" role="status" aria-live="polite">
  {#each report.warnings ?? [] as warning}
    <p class="text-warning">{warning}</p>
  {/each}
  {#if report.error}
    <p class="text-destructive">{t('tools.test_error', { error: clip(localizeError(report.error)) })}</p>
  {:else}
    <p class={cn('font-medium', report.passed ? 'text-success' : 'text-destructive')}>
      {t('tools.test_summary', { passed, total: report.cases.length })}
    </p>
    <ul class="flex flex-col gap-1">
      {#each report.cases as c, i (i)}
        <li class="flex items-start gap-1.5">
          {#if c.passed}
            <Check class="mt-0.5 size-3.5 shrink-0 text-success" aria-hidden="true" />
          {:else}
            <X class="mt-0.5 size-3.5 shrink-0 text-destructive" aria-hidden="true" />
          {/if}
          <span class="min-w-0">
            <span class="font-mono">{c.name}</span>
            {#if !c.passed}
              <span class="block text-muted-foreground">
                {t(`tools.test_r_${c.reason}` as TKey, { expected: clip(c.expected), actual: clip(c.actual) })}
              </span>
            {/if}
          </span>
        </li>
      {/each}
    </ul>
  {/if}
</div>
