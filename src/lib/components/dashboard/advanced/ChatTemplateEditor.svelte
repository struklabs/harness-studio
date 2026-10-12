<script lang="ts">
  // Custom chat template: pick a Jinja file or write one here (starting from the template
  // embedded in the GGUF) and save it as a file the definition points at (--chat-template-file).
  import { invoke } from '$lib/ipc';
  import { t } from '$lib/i18n.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Textarea } from '$lib/components/ui/textarea';
  import Field from '../parts/Field.svelte';
  import NativeSelect from '../parts/NativeSelect.svelte';

  interface Props {
    /** Path of the template file (`chat_template_file`); '' = the model's own template. */
    file: string;
    /** GGUF of the model, to show its embedded template. */
    modelPath?: string;
    /** `--jinja` is explicitly off, so a custom template may have no effect. */
    jinjaOff?: boolean;
    /** Suggested name for a new template. */
    suggestedName?: string;
  }

  let { file = $bindable(), modelPath = '', jinjaOff = false, suggestedName = '' }: Props = $props();

  interface SavedTemplate {
    name: string;
    path: string;
  }

  const uid = $props.id();
  let text = $state('');
  let name = $state('');
  let saved = $state<SavedTemplate[]>([]);
  let busy = $state(false);
  let message = $state<string | null>(null);
  let error = $state<string | null>(null);

  async function refresh() {
    try {
      saved = await invoke<SavedTemplate[]>('list_chat_templates');
    } catch {
      saved = [];
    }
  }
  $effect(() => {
    void refresh();
  });

  async function run(task: () => Promise<void>) {
    if (busy) return;
    busy = true;
    error = null;
    message = null;
    try {
      await task();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function loadFile(path: string) {
    text = await invoke<string>('read_chat_template', { path });
    name = path.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, '') ?? '';
  }

  const choose = () =>
    run(async () => {
      const path = await invoke<string | null>('pick_chat_template_file', { label: t('tpl.file_filter') });
      if (!path) return;
      file = path;
      await loadFile(path);
    });

  const open = () => run(() => loadFile(file));

  const loadEmbedded = () =>
    run(async () => {
      const embedded = await invoke<string | null>('read_model_chat_template', { path: modelPath });
      if (embedded == null) {
        error = t('tpl.no_embedded');
        return;
      }
      text = embedded;
      if (!name) name = suggestedName;
      message = t('tpl.embedded_loaded');
    });

  const save = () =>
    run(async () => {
      const s = await invoke<SavedTemplate>('save_chat_template', { name: name.trim() || suggestedName || 'template', text });
      file = s.path;
      name = s.name;
      message = t('tpl.saved', { path: s.path });
      await refresh();
    });

  const pickSaved = (path: string) =>
    run(async () => {
      file = path;
      if (path) await loadFile(path);
    });
</script>

<div class="flex flex-col gap-2">
  <Field label={t('param.chat_template_file')} id="{uid}-file" help={t('param.chat_template_file_help')} flag="--chat-template-file">
    <div class="flex gap-1.5">
      <Input id="{uid}-file" type="text" class="min-w-0 flex-1 font-mono text-xs" bind:value={file} placeholder="template.jinja" />
      <Button type="button" variant="outline" size="sm" disabled={busy} onclick={choose}>{t('tpl.choose')}</Button>
    </div>
  </Field>
  {#if saved.length > 0}
    <Field label={t('tpl.saved_list')} id="{uid}-saved">
      <NativeSelect id="{uid}-saved" value={saved.some((s) => s.path === file) ? file : ''} onchange={(e) => pickSaved(e.currentTarget.value)}>
        <option value="">{t('tpl.saved_none')}</option>
        {#each saved as s (s.path)}<option value={s.path}>{s.name}</option>{/each}
      </NativeSelect>
    </Field>
  {/if}
  {#if jinjaOff && file.trim()}
    <p class="text-xs leading-snug text-warning" role="status">{t('param.tpl_jinja_off')}</p>
  {/if}

  <Field label={t('tpl.editor')} id="{uid}-text" hint={t('tpl.editor_hint')}>
    <Textarea
      id="{uid}-text"
      rows={12}
      spellcheck={false}
      class="min-h-48 font-mono text-xs leading-snug whitespace-pre"
      placeholder={'{%- for message in messages -%}\n  …\n{%- endfor -%}'}
      bind:value={text}
    />
  </Field>
  <div class="flex flex-wrap items-center gap-1.5">
    {#if modelPath}
      <Button type="button" variant="outline" size="sm" disabled={busy} onclick={loadEmbedded}>{t('tpl.load_embedded')}</Button>
    {/if}
    <Button type="button" variant="outline" size="sm" disabled={busy || !file.trim()} onclick={open}>{t('tpl.open_file')}</Button>
    <Input type="text" class="h-7 w-40 min-w-0 font-mono text-xs" bind:value={name} placeholder={suggestedName || 'template'} aria-label={t('tpl.name')} />
    <Button type="button" size="sm" disabled={busy || !text.trim()} onclick={save}>{t('tpl.save')}</Button>
  </div>
  {#if message}<p class="text-xs leading-snug text-muted-foreground" role="status">{message}</p>{/if}
  {#if error}<p class="text-xs leading-snug text-destructive" role="alert">{error}</p>{/if}
</div>
