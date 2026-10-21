<script lang="ts">
  import { Download, CircleCheck, TriangleAlert } from '@lucide/svelte';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { errorText } from '#lib/errors.js';
  import { formatBytes } from '#lib/format.js';
  import { training } from '#lib/stores/training.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Progress } from '#lib/components/ui/progress/index.js';

  let env = $derived(training.env);
  let progress = $derived(env?.progress ?? null);
  let percent = $derived(progress && progress.total_bytes > 0 ? Math.min(100, (progress.downloaded_bytes / progress.total_bytes) * 100) : null);
</script>

{#if env}
  <section class="rounded-lg border border-border p-4" aria-label={t('training.env_title')}>
    <div class="flex flex-wrap items-center gap-3">
      {#if env.ready}
        <CircleCheck class="size-4 text-emerald-600 dark:text-emerald-400" aria-hidden="true" />
        <span class="text-sm font-medium">{t('training.env_ready')}</span>
      {:else if !env.supported}
        <TriangleAlert class="size-4 text-destructive" aria-hidden="true" />
        <span class="text-sm">{env.refusal ? errorText(env.refusal) : t('training.env_unsupported')}</span>
      {:else}
        <div class="min-w-0 flex-1">
          <p class="text-sm font-medium">{t('training.env_title')}</p>
          <p class="text-xs text-muted-foreground">
            {t(env.installed ? 'training.env_outdated' : 'training.env_needed', { size: formatBytes(env.estimated_size_bytes, 1) })}
          </p>
        </div>
        <Button disabled={training.installing} onclick={() => training.installEnv()}>
          <Download class="size-4" aria-hidden="true" />
          {t(training.installing ? 'training.env_installing' : 'training.env_install')}
        </Button>
      {/if}
    </div>
    {#if training.installing || progress}
      <div class="mt-3 flex flex-col gap-1">
        <Progress value={percent ?? 0} />
        <p class="truncate text-xs text-muted-foreground">
          {progress ? t(`training.env_step_${progress.step}` as TKey) : ''}
          {#if percent !== null}· {formatBytes(progress?.downloaded_bytes, 1)} / {formatBytes(progress?.total_bytes, 1)}{/if}
          {#if progress?.detail}· {progress.detail}{/if}
        </p>
      </div>
    {/if}
    {#if training.installError}
      <p class="mt-2 text-xs text-destructive" role="alert">{training.installError}</p>
    {/if}
  </section>
{/if}
