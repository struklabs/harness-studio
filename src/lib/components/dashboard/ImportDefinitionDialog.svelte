<script lang="ts">
  // Import of a `*.harness.toml`: shows what was found on this machine (model file, other
  // files, embedded preset), lets the user pick a free alias, then saves.
  import { invoke } from '#lib/ipc.js';
  import type { DefinitionImport, ModelDefinition } from '#lib/types.js';
  import { t } from '#lib/i18n.svelte.js';
  import { domain } from '#lib/stores/domain.svelte.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { CircleCheck, TriangleAlert } from '@lucide/svelte';
  import Field from './parts/Field.svelte';

  interface Props {
    preview: DefinitionImport;
    onImported: (def: ModelDefinition) => void;
    onClose: () => void;
  }

  let { preview, onImported, onClose }: Props = $props();

  const uid = $props.id();
  const initialAlias = () => preview.suggested_alias;
  let alias = $state(initialAlias());
  let busy = $state(false);
  let error = $state<string | null>(null);

  let taken = $derived(domain.definitions.some((d) => d.alias.toLowerCase() === alias.trim().toLowerCase()));
  let base = $derived(preview.definition.base);

  async function doImport() {
    busy = true;
    error = null;
    try {
      const def = await invoke<ModelDefinition>('import_model_definition', { import: preview, alias: alias.trim() });
      await Promise.all([domain.refreshDefinitions(), domain.refreshPresets()]);
      onImported(def);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<Dialog.Root open={true} onOpenChange={(open) => !open && !busy && onClose()}>
  <Dialog.Content class="sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title>{t('def.import_title')}</Dialog.Title>
      <Dialog.Description>{t('def.import_desc')}</Dialog.Description>
    </Dialog.Header>
    <div class="flex flex-col gap-3 text-xs">
      <Field help={t('help.alias')} label={t('def.alias')} id="{uid}-alias" note={taken ? t('def.alias_taken', { alias: alias.trim() }) : preview.alias_taken ? t('def.import_alias_suggested') : undefined}>
        <Input id="{uid}-alias" class="font-mono" bind:value={alias} aria-invalid={taken ? 'true' : undefined} />
      </Field>
      <div class="flex items-start gap-2">
        {#if preview.model_found}
          <CircleCheck class="mt-px size-4 shrink-0 text-success" />
          <span class="min-w-0 break-all font-mono text-xs">{base.kind === 'local' ? base.path : ''}</span>
        {:else}
          <TriangleAlert class="mt-px size-4 shrink-0 text-warning" />
          <span class="min-w-0">
            {t('def.import_model_missing')}
            {#if preview.hf_ref}<span class="block break-all font-mono text-xs text-muted-foreground">{preview.hf_ref}</span>{/if}
          </span>
        {/if}
      </div>
      {#if preview.missing.length}
        <div class="flex items-start gap-2">
          <TriangleAlert class="mt-px size-4 shrink-0 text-warning" />
          <span class="min-w-0">
            {t('def.import_files_missing')}
            {#each preview.missing as m (m)}<span class="block break-all font-mono text-xs text-muted-foreground">{m}</span>{/each}
          </span>
        </div>
      {/if}
      {#if preview.preset}
        <p class="text-muted-foreground">{t('def.import_preset', { name: preview.preset.name })}</p>
      {/if}
      {#if error}
        <p class="rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-2 text-destructive" role="alert">{error}</p>
      {/if}
    </div>
    <Dialog.Footer>
      <Button variant="outline" onclick={onClose} disabled={busy}>{t('ip.cancel')}</Button>
      <Button onclick={doImport} disabled={busy || taken || !alias.trim()}>{t('def.import_yes')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
