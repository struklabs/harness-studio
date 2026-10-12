<script lang="ts">
  // Import of an Ollama Modelfile: previews what was mapped (values, model file) and what could
  // not be carried over (warnings), lets the user pick a free alias, then saves the definition
  // and its preset through the same command as a `.harness.toml` import.
  import { invoke } from '#lib/ipc.js';
  import type { ModelfileImport, ModelDefinition } from '#lib/types.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { domain } from '#lib/stores/domain.svelte.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { CircleCheck, TriangleAlert } from '@lucide/svelte';
  import Field from './parts/Field.svelte';

  interface Props {
    preview: ModelfileImport;
    onImported: (def: ModelDefinition) => void;
    onClose: () => void;
  }

  let { preview, onImported, onClose }: Props = $props();

  const uid = $props.id();
  const initialAlias = () => preview.import.suggested_alias;
  let alias = $state(initialAlias());
  let busy = $state(false);
  let error = $state<string | null>(null);

  let taken = $derived(domain.definitions.some((d) => d.alias.toLowerCase() === alias.trim().toLowerCase()));
  let base = $derived(preview.import.definition.base);

  function warningText(w: { code: string; line: number | null; detail: string }): string {
    const text = t(`mf.w_${w.code}` as TKey, { detail: w.detail });
    return w.line ? `${t('mf.line', { line: w.line })}: ${text}` : text;
  }

  async function doImport() {
    busy = true;
    error = null;
    try {
      const def = await invoke<ModelDefinition>('import_model_definition', { import: preview.import, alias: alias.trim() });
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
  <Dialog.Content class="sm:max-w-2xl">
    <Dialog.Header>
      <Dialog.Title>{t('mf.title')}</Dialog.Title>
      <Dialog.Description>{t('mf.desc')}</Dialog.Description>
    </Dialog.Header>
    <div class="flex max-h-[60vh] flex-col gap-3 overflow-y-auto text-xs">
      <Field help={t('help.alias')} label={t('def.alias')} id="{uid}-alias" note={taken ? t('def.alias_taken', { alias: alias.trim() }) : preview.import.alias_taken ? t('def.import_alias_suggested') : undefined}>
        <Input id="{uid}-alias" class="font-mono" bind:value={alias} aria-invalid={taken ? 'true' : undefined} />
      </Field>
      <div class="flex items-start gap-2">
        {#if preview.import.model_found}
          <CircleCheck class="mt-px size-4 shrink-0 text-success" />
        {:else}
          <TriangleAlert class="mt-px size-4 shrink-0 text-warning" />
        {/if}
        <span class="min-w-0">
          <span class="block text-muted-foreground">{t('mf.from', { from: preview.from })}</span>
          <span class="block break-all font-mono text-xs">{base.kind === 'local' ? base.path : ''}</span>
        </span>
      </div>
      {#if preview.import.missing.length}
        <div class="flex items-start gap-2">
          <TriangleAlert class="mt-px size-4 shrink-0 text-warning" />
          <span class="min-w-0">
            {t('def.import_files_missing')}
            {#each preview.import.missing as m (m)}<span class="block break-all font-mono text-xs text-muted-foreground">{m}</span>{/each}
          </span>
        </div>
      {/if}
      <section>
        <h3 class="mb-1 font-medium">{t('mf.mapped')}</h3>
        <div class="overflow-x-auto rounded-md border">
          <table class="w-full text-left text-xs">
            <thead class="bg-muted/50 text-muted-foreground">
              <tr>
                <th class="px-2 py-1 font-medium">{t('mf.col_source')}</th>
                <th class="px-2 py-1 font-medium">{t('mf.col_target')}</th>
                <th class="px-2 py-1 font-medium">{t('mf.col_value')}</th>
              </tr>
            </thead>
            <tbody>
              {#each preview.mapped as m, i (i)}
                <tr class="border-t align-top">
                  <td class="max-w-56 break-all px-2 py-1 font-mono">{m.source}</td>
                  <td class="px-2 py-1 font-mono">{m.target}</td>
                  <td class="break-all px-2 py-1 font-mono">{m.value}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>
      <section>
        <h3 class="mb-1 font-medium">{t('mf.warnings')}</h3>
        {#if preview.warnings.length}
          <ul class="flex flex-col gap-1.5">
            {#each preview.warnings as w, i (i)}
              <li class="flex items-start gap-2">
                <TriangleAlert class="mt-px size-3.5 shrink-0 text-warning" />
                <span class="min-w-0 break-words">{warningText(w)}</span>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="text-muted-foreground">{t('mf.no_warnings')}</p>
        {/if}
      </section>
      {#if preview.import.preset}
        <p class="text-muted-foreground">{t('def.import_preset', { name: preview.import.preset.name })}</p>
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
