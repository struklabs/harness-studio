<script lang="ts">
  // Deleting a model definition: blocked while a server profile serves it (the profiles are
  // listed); otherwise confirmed.
  import { invoke } from '$lib/ipc';
  import { onMount } from 'svelte';
  import type { DefinitionRefs, ModelDefinition } from '$lib/types';
  import { t } from '$lib/i18n.svelte';
  import { domain } from '$lib/stores/domain.svelte';
  import * as Dialog from '$lib/components/ui/dialog';
  import { Button } from '$lib/components/ui/button';

  interface Props {
    definition: ModelDefinition;
    onDeleted: () => void;
    onClose: () => void;
  }

  let { definition, onDeleted, onClose }: Props = $props();

  let refs = $state<DefinitionRefs | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  onMount(async () => {
    try {
      refs = await invoke<DefinitionRefs>('get_definition_refs', { id: definition.id });
    } catch (e) {
      error = String(e);
    }
  });

  let blocked = $derived(!!refs && refs.profiles.length > 0);

  async function remove() {
    busy = true;
    error = null;
    try {
      await invoke('delete_model_definition', { id: definition.id });
      await domain.refreshDefinitions();
      onDeleted();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<Dialog.Root open={true} onOpenChange={(open) => !open && !busy && onClose()}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t('def.delete_title', { alias: definition.alias })}</Dialog.Title>
      <Dialog.Description>
        {blocked ? t('def.delete_blocked') : t('def.delete_desc')}
      </Dialog.Description>
    </Dialog.Header>
    {#if refs}
      {#if blocked}
        <ul class="flex flex-col gap-1 rounded-md border border-border p-2 text-xs">
          {#each refs.profiles as p (p.id)}
            <li class="flex items-center gap-2"><span class="truncate">{p.name}</span><span class="font-mono text-muted-foreground">{p.id}</span></li>
          {/each}
        </ul>
      {:else if refs.model_default_for.length || refs.session}
        <p class="text-xs text-muted-foreground">{t('def.delete_default_note')}</p>
      {/if}
    {/if}
    {#if error}
      <p class="rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-2 text-xs text-destructive" role="alert">{error}</p>
    {/if}
    <Dialog.Footer>
      <Button variant="outline" onclick={onClose} disabled={busy}>{blocked ? t('def.close') : t('ip.cancel')}</Button>
      {#if !blocked}
        <Button variant="destructive" onclick={remove} disabled={busy || !refs}>{t('def.delete_yes')}</Button>
      {/if}
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
