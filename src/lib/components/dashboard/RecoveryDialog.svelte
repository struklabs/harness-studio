<script lang="ts">
  // After a start failed with out-of-memory: the backend's recovery options (lower context /
  // GPU layers / -ub, quantized KV cache, --fit, MoE experts on the CPU, stop another server),
  // each with concrete values. "Retry" is a real start with those values as a transient layer;
  // they are saved to the model definition only when the checkbox is on.
  import { invoke } from '$lib/ipc';
  import { onMount } from 'svelte';
  import { toast } from 'svelte-sonner';
  import type { RecoveryOption, RecoveryPlan } from '$lib/types';
  import { t, type TKey } from '$lib/i18n.svelte';
  import { localizeError } from '$lib/network';
  import { failureText, gib } from '$lib/failure';
  import { app } from '$lib/stores/app.svelte';
  import * as Dialog from '$lib/components/ui/dialog';
  import { Button } from '$lib/components/ui/button';
  import { Badge } from '$lib/components/ui/badge';
  import { Checkbox } from '$lib/components/ui/checkbox';
  import { LoaderCircle, RotateCcw, TriangleAlert } from '@lucide/svelte';

  interface Props {
    serverId: string;
    /** Called after a retry started (with the values it used), e.g. to update the form. */
    onRetried?: (option: RecoveryOption, saved: boolean) => void;
    onClose: () => void;
  }

  let { serverId, onRetried, onClose }: Props = $props();

  const uid = $props.id();
  let plan = $state<RecoveryPlan | null>(null);
  let error = $state<string | null>(null);
  let busyId = $state<string | null>(null);
  let save = $state(false);

  const STAGE: Record<string, TKey> = {
    kv_cache: 'recovery.stage_kv_cache',
    weights: 'recovery.stage_weights',
    compute: 'recovery.stage_compute',
  };

  onMount(async () => {
    try {
      plan = await invoke<RecoveryPlan>('get_recovery_plan', { serverId });
    } catch (e) {
      error = localizeError(e);
    }
  });

  function label(o: RecoveryOption): string {
    const params = { ...o.params };
    if (o.kind === 'unload_server' && params.server_id) params.server = app.serverLabel(params.server_id) || params.server;
    return t(o.label_key as TKey, params);
  }

  async function retry(o: RecoveryOption) {
    busyId = o.id;
    error = null;
    try {
      await invoke('retry_server', { serverId, overrides: o.overrides, unload: o.unload, save });
      toast.success(t('recovery.retry_started'));
      onRetried?.(o, save);
      await app.refreshServer();
      onClose();
    } catch (e) {
      error = t('recovery.retry_failed', { error: localizeError(e) });
    } finally {
      busyId = null;
    }
  }

  let stage = $derived(plan?.failure.params.stage ? STAGE[plan.failure.params.stage] : null);
</script>

<Dialog.Root open={true} onOpenChange={(open) => !open && !busyId && onClose()}>
  <Dialog.Content class="flex max-h-[85vh] flex-col sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title>{t('recovery.dialog_title')}</Dialog.Title>
      <Dialog.Description>{t('recovery.dialog_desc')}</Dialog.Description>
    </Dialog.Header>

    {#if !plan && !error}
      <p class="flex items-center gap-2 text-xs text-muted-foreground" role="status">
        <LoaderCircle class="size-3.5 animate-spin" />{t('recovery.loading')}
      </p>
    {/if}

    {#if plan}
      <div class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto text-xs">
        <div class="flex flex-col gap-1 rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-2 text-destructive">
          <span>{failureText(plan.failure)}</span>
          {#if stage}<span class="text-muted-foreground">{t(stage)}</span>{/if}
          {#if plan.free_vram_bytes !== null || plan.estimated_need_bytes !== null}
            <span class="font-num text-muted-foreground">
              {t('recovery.memory_line', { free: gib(plan.free_vram_bytes), need: gib(plan.estimated_need_bytes) })}
            </span>
          {/if}
        </div>

        {#if plan.options.length === 0}
          <p class="text-muted-foreground">{t('recovery.no_options')}</p>
        {:else}
          <ul class="flex flex-col gap-1.5" aria-label={t('failure.recover')}>
            {#each plan.options as o (o.id)}
              <li class="flex items-center gap-2 rounded-md border border-border px-2.5 py-2">
                <div class="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span class="break-words">{label(o)}</span>
                  {#if o.warning_key}
                    <span class="flex items-start gap-1 text-xs text-warning"><TriangleAlert class="mt-px size-3.5 shrink-0" />{t(o.warning_key as TKey, o.params)}</span>
                  {/if}
                  {#if o.recommended}
                    <Badge variant="outline" class="self-start border-border bg-muted text-xs text-muted-foreground">{t('recovery.recommended')}</Badge>
                  {/if}
                </div>
                <Button size="sm" variant={o.recommended ? 'default' : 'outline'} disabled={busyId !== null} onclick={() => retry(o)}>
                  {#if busyId === o.id}<LoaderCircle class="animate-spin" />{t('recovery.retrying')}{:else}<RotateCcw />{t('recovery.retry')}{/if}
                </Button>
              </li>
            {/each}
          </ul>
          <label class="flex items-start gap-2" for="{uid}-save">
            <Checkbox id="{uid}-save" bind:checked={save} class="mt-0.5" aria-describedby="{uid}-save-note" />
            <span class="flex flex-col gap-0.5">
              <span>{t('recovery.save_label')}</span>
              <span id="{uid}-save-note" class="text-muted-foreground">{t('recovery.save_hint')}</span>
            </span>
          </label>
        {/if}
      </div>
    {/if}

    {#if error}
      <p class="rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-2 text-xs text-destructive" role="alert">{error}</p>
    {/if}
    <Dialog.Footer>
      <Button variant="outline" onclick={onClose} disabled={busyId !== null}>{t('def.close')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
