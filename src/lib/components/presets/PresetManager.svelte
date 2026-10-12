<script lang="ts">
  import AdvancedPageNotice from '../common/AdvancedPageNotice.svelte';
  import HelpPopover from '../common/HelpPopover.svelte';
  // Inference preset manager: built-ins (read-only, "Copy" makes an editable copy) and the
  // user's presets (create, edit, rename, duplicate, delete, import/export JSON).
  import { invoke } from '$lib/ipc';
  import { toast } from 'svelte-sonner';
  import type { InferencePreset } from '$lib/types';
  import { presetName, presetDescription } from '$lib/components/presets/presetText';
  import { t } from '$lib/i18n.svelte';
  import { cn } from '$lib/utils';
  import { domain } from '$lib/stores/domain.svelte';
  import * as Dialog from '$lib/components/ui/dialog';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Textarea } from '$lib/components/ui/textarea';
  import Field from '$lib/components/dashboard/parts/Field.svelte';
  import { Plus, Copy, Download, Upload, Trash2, Lock } from '@lucide/svelte';
  import RequestParamsEditor from './RequestParamsEditor.svelte';
  import { cleanRequest, requestOf, sameRequest, type PresetRequest } from './request';

  const uid = $props.id();

  let selectedId = $state<string | null>(null);
  let name = $state('');
  let description = $state('');
  let systemPrompt = $state('');
  let request = $state<PresetRequest>({ sampling: {}, reasoning: {} });
  let saving = $state(false);
  let error = $state<string | null>(null);
  let confirmDelete = $state(false);
  /** Leaving unsaved edits (another preset or closing) waits for a confirmation. */
  let pending = $state<{ kind: 'select'; preset: InferencePreset } | { kind: 'close' } | null>(null);

  let selected = $derived(domain.preset(selectedId));
  let builtins = $derived(domain.presets.filter((p) => p.builtin));
  let users = $derived(domain.presets.filter((p) => !p.builtin));
  let readOnly = $derived(!!selected?.builtin);
  let dirty = $derived(
    !!selected &&
      !selected.builtin &&
      (name.trim() !== selected.name ||
        description.trim() !== (selected.description ?? '') ||
        systemPrompt !== (selected.system_prompt ?? '') ||
        !sameRequest(request, requestOf(selected)))
  );

  // Opening: refresh the list and select the requested (or first) preset.
  $effect(() => {
    if (!domain.presetManagerOpen) return;
    const want = domain.presetManagerSelect;
    void domain.refreshPresets().then((list) => {
      const target = list.find((p) => p.id === want) ?? list.find((p) => p.id === selectedId) ?? list[0];
      if (target) load(target);
    });
  });

  function load(p: InferencePreset) {
    selectedId = p.id;
    name = presetName(p);
    description = presetDescription(p);
    systemPrompt = p.system_prompt ?? '';
    request = requestOf(p);
    error = null;
    confirmDelete = false;
    pending = null;
  }

  function select(p: InferencePreset) {
    if (dirty) pending = { kind: 'select', preset: p };
    else load(p);
  }

  function discardAndContinue() {
    const next = pending;
    pending = null;
    if (next?.kind === 'select') load(next.preset);
    else if (next?.kind === 'close') domain.presetManagerOpen = false;
  }

  async function saveAs(preset: InferencePreset, message: string): Promise<InferencePreset | null> {
    saving = true;
    error = null;
    try {
      const saved = await invoke<InferencePreset>('save_inference_preset', { preset });
      await domain.refreshPresets();
      load(saved);
      toast.success(message);
      return saved;
    } catch (e) {
      error = String(e);
      return null;
    } finally {
      saving = false;
    }
  }

  function current(id: string, n: string): InferencePreset {
    const c = cleanRequest($state.snapshot(request) as PresetRequest);
    return {
      id,
      name: n,
      description: description.trim(),
      system_prompt: systemPrompt.trim() ? systemPrompt : null,
      sampling: c.sampling,
      reasoning: c.reasoning,
      response_format: c.response_format ?? null,
      tool_choice: c.tool_choice ?? null,
      created_at: 0,
      updated_at: 0,
    };
  }

  function freeName(base: string): string {
    const taken = new Set(domain.presets.map((p) => p.name.toLowerCase()));
    if (!taken.has(base.toLowerCase())) return base;
    for (let n = 2; ; n++) {
      const c = `${base} (${n})`;
      if (!taken.has(c.toLowerCase())) return c;
    }
  }

  const save = () => selected && saveAs(current(selected.id, name.trim()), t('ip.saved', { name: name.trim() }));
  const copy = () => {
    const n = freeName(t('ip.copy_name', { name: selected ? presetName(selected) : name }));
    return saveAs(current('', n), t('ip.copied', { name: n }));
  };
  const create = () => {
    const n = freeName(t('ip.new_name'));
    const blank: InferencePreset = { id: '', name: n, sampling: {}, created_at: 0, updated_at: 0 };
    return saveAs(blank, t('ip.created', { name: n }));
  };

  async function remove() {
    if (!selected || selected.builtin) return;
    try {
      await invoke('delete_inference_preset', { id: selected.id });
      const gone = selected.name;
      await domain.refreshPresets();
      await domain.refreshDefinitions();
      const next = domain.presets[0];
      if (next) load(next);
      toast.success(t('ip.deleted', { name: gone }));
    } catch (e) {
      error = String(e);
    }
  }

  async function exportPresets(ids: string[] | null) {
    try {
      const fileName = ids?.length === 1 ? ((p) => (p ? presetName(p) : undefined))(domain.preset(ids[0])) : 'presets';
      const path = await invoke<string | null>('export_inference_presets', { ids, fileName: fileName ?? null });
      if (path) toast.success(t('ip.exported', { path }));
    } catch (e) {
      toast.error(String(e));
    }
  }

  async function importPresets() {
    try {
      const list = await invoke<InferencePreset[] | null>('import_inference_presets_dialog');
      if (!list) return;
      await domain.refreshPresets();
      if (list[0]) load(domain.preset(list[0].id) ?? list[0]);
      toast.success(t('ip.imported', { count: list.length }));
    } catch (e) {
      toast.error(t('ip.import_failed', { error: String(e) }));
    }
  }

  function close() {
    if (dirty) pending = { kind: 'close' };
    else domain.presetManagerOpen = false;
  }
</script>

{#snippet item(p: InferencePreset)}
  <button
    type="button"
    class={cn(
      'flex w-full min-w-0 items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs outline-none transition-colors hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50',
      p.id === selectedId && 'bg-muted/60 text-foreground'
    )}
    aria-current={p.id === selectedId ? 'true' : undefined}
    onclick={() => select(p)}
  >
    <span class="min-w-0 flex-1 truncate">{presetName(p)}</span>
    {#if p.builtin}<Lock class="size-3 shrink-0 text-muted-foreground" aria-label={t('ip.builtin')} />{/if}
  </button>
{/snippet}

<Dialog.Root open={domain.presetManagerOpen} onOpenChange={(open) => !open && close()}>
  <Dialog.Content class="flex max-h-[88vh] flex-col gap-0 p-0 sm:max-w-[920px]">
    <Dialog.Header class="border-b border-border px-5 py-4">
      <Dialog.Title class="flex items-center gap-1">{t('ip.title')}<HelpPopover text={t('ip.desc')} /></Dialog.Title>
      <Dialog.Description class="sr-only">{t('ip.desc')}</Dialog.Description>
    </Dialog.Header>

    <AdvancedPageNotice level="advanced" />
    <div class="grid min-h-0 flex-1 grid-cols-[220px_minmax(0,1fr)] overflow-hidden">
      <!-- List -->
      <nav class="flex min-h-0 flex-col border-r border-border" aria-label={t('ip.list_label')}>
        <div class="min-h-0 flex-1 overflow-y-auto p-2">
          <p class="px-2 pt-1 pb-1 text-xs font-medium text-muted-foreground">{t('ip.builtins')}</p>
          {#each builtins as p (p.id)}{@render item(p)}{/each}
          <p class="px-2 pt-3 pb-1 text-xs font-medium text-muted-foreground">{t('ip.mine')}</p>
          {#each users as p (p.id)}{@render item(p)}{/each}
          {#if users.length === 0}
            <p class="px-2 py-1 text-xs text-muted-foreground">{t('ip.mine_empty')}</p>
          {/if}
        </div>
        <div class="flex flex-wrap gap-1 border-t border-border p-2">
          <Button size="xs" variant="outline" onclick={create} disabled={saving}><Plus />{t('ip.new')}</Button>
          <Button size="xs" variant="ghost" onclick={importPresets}><Upload />{t('ip.import')}</Button>
          <Button size="xs" variant="ghost" onclick={() => exportPresets(null)} disabled={users.length === 0}><Download />{t('ip.export_all')}</Button>
        </div>
      </nav>

      <!-- Editor -->
      <div class="flex min-h-0 flex-col">
        {#if selected}
          <div class="min-h-0 flex-1 overflow-y-auto p-5">
            <div class="flex flex-col gap-4">
              {#if readOnly}
                <div class="flex items-center gap-2 rounded-md border border-border bg-muted/40 px-3 py-2 text-xs text-muted-foreground" role="note">
                  <Lock class="size-3.5 shrink-0" />
                  <span class="min-w-0 flex-1">{t('ip.builtin_note')}</span>
                  <Button size="xs" variant="outline" onclick={copy} disabled={saving}><Copy />{t('ip.copy')}</Button>
                </div>
              {/if}
              <div class="grid grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)] gap-3">
                <Field label={t('ip.name')} id="{uid}-name">
                  <Input id="{uid}-name" maxlength={80} bind:value={name} disabled={readOnly} />
                </Field>
                <Field label={t('ip.description')} id="{uid}-desc">
                  <Input id="{uid}-desc" maxlength={200} bind:value={description} disabled={readOnly} />
                </Field>
              </div>
              <Field label={t('ip.system_prompt')} id="{uid}-sys">
                <Textarea
                  id="{uid}-sys"
                  rows={3}
                  class="min-h-16 resize-y text-xs"
                  placeholder={t('ip.system_prompt_placeholder')}
                  bind:value={systemPrompt}
                  disabled={readOnly}
                />
              </Field>
              <div class="flex items-center gap-2">
                <span class="text-sm font-semibold text-foreground">{t('ip.request')}</span>
                {#if Object.keys(cleanRequest(request).sampling).length === 0 && !readOnly}
                  <span class="text-xs text-muted-foreground">{t('ip.sends_nothing')}</span>
                {/if}
              </div>
              <RequestParamsEditor bind:request disabled={readOnly} />
            </div>
          </div>
          {#if error}
            <p class="mx-5 mb-2 rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-2 text-xs text-destructive" role="alert">{error}</p>
          {/if}
          {#if pending}
            <div class="mx-5 mb-2 flex items-center gap-2 rounded-md border border-warning/30 bg-warning/10 px-2.5 py-2 text-xs" role="alert">
              <span class="min-w-0 flex-1">{t('ip.discard_changes')}</span>
              <Button size="xs" variant="ghost" onclick={() => (pending = null)}>{t('ip.cancel')}</Button>
              <Button size="xs" variant="outline" onclick={discardAndContinue}>{t('ip.discard')}</Button>
            </div>
          {/if}
          <div class="flex items-center gap-2 border-t border-border px-5 py-3">
            {#if !readOnly}
              {#if confirmDelete}
                <span class="text-xs text-destructive">{t('ip.delete_confirm', { name: presetName(selected) })}</span>
                <Button size="sm" variant="destructive" onclick={remove}>{t('ip.delete_yes')}</Button>
                <Button size="sm" variant="ghost" onclick={() => (confirmDelete = false)}>{t('ip.cancel')}</Button>
              {:else}
                <Button size="sm" variant="ghost" onclick={() => (confirmDelete = true)}><Trash2 />{t('ip.delete')}</Button>
                <Button size="sm" variant="ghost" onclick={copy} disabled={saving}><Copy />{t('ip.duplicate')}</Button>
              {/if}
            {/if}
            <Button size="sm" variant="ghost" onclick={() => selected && exportPresets([selected.id])}><Download />{t('ip.export')}</Button>
            <span class="flex-1"></span>
            {#if !readOnly}
              {#if dirty}<span class="text-xs text-warning">{t('ip.unsaved')}</span>{/if}
              <Button size="sm" onclick={save} disabled={saving || !dirty || !name.trim()}>{saving ? t('ip.saving') : t('ip.save')}</Button>
            {/if}
          </div>
        {:else}
          <p class="p-5 text-xs text-muted-foreground">{t('ip.pick')}</p>
        {/if}
      </div>
    </div>
  </Dialog.Content>
</Dialog.Root>
