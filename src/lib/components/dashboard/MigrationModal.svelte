<script lang="ts">
  import { formatBytes } from '#lib/format.js';
  import type { LocalModel, StorageDirInfo } from '#lib/types.js';
  import { invoke } from '#lib/ipc.js';
  import { onDestroy } from 'svelte';
  import { t } from '#lib/i18n.svelte.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import Field from './parts/Field.svelte';
  import NativeSelect from './parts/NativeSelect.svelte';

  interface Props {
    /** Model being moved; may be briefly undefined while the list refreshes after a move. */
    model: LocalModel | undefined;
    storageDirs: StorageDirInfo[];
    /** Called with the model's new id once the move succeeded (selects it and rescans). */
    onMigrated: (migratedId: string) => Promise<void>;
    onClose: () => void;
  }

  let { model, storageDirs, onMigrated, onClose }: Props = $props();

  const uid = $props.id();

  let availableTargets = $derived(storageDirs.filter((d) => d.path !== model?.root_dir && d.exists));

  let isMigrating = $state<boolean>(false);
  let migrationTarget = $state<string>(firstTarget());
  let migrationMsg = $state<string | null>(null);
  let migrationError = $state<string | null>(null);
  let migrationCloseTimer: ReturnType<typeof setTimeout> | undefined;

  onDestroy(() => clearTimeout(migrationCloseTimer));

  /** Target preselected when the modal opens. */
  function firstTarget(): string {
    return availableTargets.length > 0 ? availableTargets[0].path : '';
  }

  function closeMigrationModal() {
    if (isMigrating) return;
    onClose();
  }

  async function handleMigrateModel() {
    if (!model || !migrationTarget || isMigrating) return;
    isMigrating = true;
    migrationError = null;
    migrationMsg = t('dash.mm_progress_msg');

    try {
      const migratedId: string = await invoke('migrate_model', {
        modelId: model.id,
        targetDir: migrationTarget,
      });
      migrationMsg = t('dash.mm_success_msg');
      await onMigrated(migratedId);
      clearTimeout(migrationCloseTimer);
      migrationCloseTimer = setTimeout(closeMigrationModal, 1500);
    } catch (e: any) {
      migrationError = t('dash.mm_error_msg', { error: e });
      migrationMsg = null;
    } finally {
      isMigrating = false;
    }
  }
</script>

<Dialog.Root open={!!model} onOpenChange={(open) => !open && closeMigrationModal()}>
  <Dialog.Content class="sm:max-w-lg" showCloseButton={!isMigrating}>
    {#if model}
      <Dialog.Header>
        <Dialog.Title>{t('dash.mm_title')}</Dialog.Title>
        <Dialog.Description>
          <strong class="font-medium text-foreground">{model.name}</strong>
          {t('dash.mm_desc_suffix')}
        </Dialog.Description>
      </Dialog.Header>

      <div class="flex flex-col gap-2 rounded-md border border-border bg-muted/30 px-3 py-2 text-xs">
        <div class="flex min-w-0 gap-2">
          <span class="shrink-0 text-muted-foreground">{t('dash.mm_current_dir_label')}</span>
          <code class="min-w-0 truncate font-mono" title={model.root_dir}>{model.root_dir}</code>
        </div>
        <div class="flex gap-2">
          <span class="shrink-0 text-muted-foreground">{t('dash.mm_required_space_label')}</span>
          <span class="font-num">{formatBytes(model.size_bytes)}</span>
        </div>
      </div>

      <Field help={t('help.migration')} label={t('dash.mm_target_label')} id="{uid}-dest">
        {#if availableTargets.length === 0}
          <p class="rounded-md border border-warning/30 bg-warning/10 px-2.5 py-2 text-xs text-warning">{t('dash.mm_no_targets')}</p>
        {:else}
          <NativeSelect id="{uid}-dest" bind:value={migrationTarget} disabled={isMigrating}>
            {#each availableTargets as dir (dir.path)}
              <option value={dir.path}>
                {dir.path} ({dir.is_primary ? t('dash.mm_primary') : t('dash.mm_extra_dir')}) — {t('dash.mm_free_space')} {formatBytes(dir.free_bytes)}
              </option>
            {/each}
          </NativeSelect>
        {/if}
      </Field>

      {#if migrationMsg}
        <p class="text-xs text-muted-foreground" role="status">{migrationMsg}</p>
      {/if}
      {#if migrationError}
        <p class="rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-2 text-xs text-destructive" role="alert">{migrationError}</p>
      {/if}

      <Dialog.Footer>
        <Button variant="outline" onclick={closeMigrationModal} disabled={isMigrating}>{t('dash.mm_cancel_btn')}</Button>
        <Button onclick={handleMigrateModel} disabled={isMigrating || !migrationTarget || availableTargets.length === 0}>
          {isMigrating ? t('dash.mm_moving_btn') : t('dash.migrate_model')}
        </Button>
      </Dialog.Footer>
    {/if}
  </Dialog.Content>
</Dialog.Root>
