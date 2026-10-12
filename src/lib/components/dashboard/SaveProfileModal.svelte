<script lang="ts">
  import { invoke } from '$lib/ipc';
  import type { LaunchConfig, ServerProfile } from '$lib/types';
  import { app } from '$lib/stores/app.svelte';
  import { t } from '$lib/i18n.svelte';
  import * as Dialog from '$lib/components/ui/dialog';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Checkbox } from '$lib/components/ui/checkbox';
  import Field from './parts/Field.svelte';
  import { makeProfileId } from '../servers/utils';

  interface Props {
    /** Launch config exactly as the Dashboard would send it to `start_server`. */
    config: LaunchConfig;
    modelId: string | null;
    defaultName: string;
    defaultDescription?: string;
    onSaved: (name: string) => void;
    onClose: () => void;
  }

  let { config, modelId, defaultName, defaultDescription = '', onSaved, onClose }: Props = $props();

  const uid = $props.id();

  /** Prefill only; the user edits the name from here on. */
  const initialName = () => defaultName;
  const initialDescription = () => defaultDescription;

  let name = $state(initialName());
  let description = $state(initialDescription());
  let autoStart = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);

  function close() {
    if (!saving) onClose();
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    if (saving) return;
    const trimmed = name.trim();
    if (!trimmed) {
      error = t('srv.name_required');
      return;
    }
    saving = true;
    error = null;
    const profile: ServerProfile = {
      id: makeProfileId(trimmed, [...app.profiles.map((p) => p.id), ...app.servers.map((s) => s.id)]),
      name: trimmed,
      model_id: modelId,
      description: description.trim(),
      config,
      auto_start: autoStart,
      updated_at: 0,
    };
    try {
      app.setProfiles(await invoke<ServerProfile[]>('save_server_profile', { profile }));
      saving = false;
      onSaved(trimmed);
    } catch (err) {
      error = String(err || t('srv.error_generic'));
      saving = false;
    }
  }
</script>

<Dialog.Root open={true} onOpenChange={(open) => !open && close()}>
  <Dialog.Content class="sm:max-w-md" showCloseButton={!saving}>
    <Dialog.Header>
      <Dialog.Title>{t('srv.save_modal_title')}</Dialog.Title>
      <Dialog.Description>{t('srv.save_modal_desc')}</Dialog.Description>
    </Dialog.Header>

    <form onsubmit={save} class="flex flex-col gap-3">
      <Field label={t('srv.name_label')} id="{uid}-name">
        <Input
          id="{uid}-name"
          type="text"
          maxlength={80}
          placeholder={t('srv.name_placeholder')}
          bind:value={name}
          aria-invalid={error ? 'true' : undefined}
          aria-describedby={error ? `${uid}-error` : undefined}
        />
      </Field>

      <Field help={t('help.description')} label={t('srv.desc_label')} id="{uid}-desc">
        <Input id="{uid}-desc" type="text" maxlength={200} placeholder={t('srv.desc_placeholder')} bind:value={description} />
      </Field>

      <Field label={t('srv.auto_start')} id="{uid}-auto-start" help={t('help.auto_start')} note={t('srv.auto_start_note')}>
        <Checkbox id="{uid}-auto-start" bind:checked={autoStart} />
      </Field>

      {#if error}
        <p class="rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-2 text-xs text-destructive" id="{uid}-error" role="alert">{error}</p>
      {/if}

      <Dialog.Footer>
        <Button type="button" variant="outline" onclick={close} disabled={saving}>{t('srv.cancel')}</Button>
        <Button type="submit" disabled={saving}>{saving ? t('srv.saving') : t('srv.save')}</Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
