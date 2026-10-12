<script lang="ts">
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import HelpPopover from '../common/HelpPopover.svelte';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  import { untrack } from 'svelte';
  import { ShieldAlert } from '@lucide/svelte';
  import { Button } from '#lib/components/ui/button/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import * as Field from '#lib/components/ui/field/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import * as ToggleGroup from '#lib/components/ui/toggle-group/index.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { MCP_NAME_RE, formatKeyValueLines, parseKeyValueLines } from '#lib/mcp-client.js';
  import { MCP_TRANSPORTS, type McpServerDef, type McpTransport } from '#lib/mcp-types.js';
  import NativeSelect from '../dashboard/parts/NativeSelect.svelte';

  // Rendered inside <Dialog.Content>; the parent owns the dialog's open state.
  interface Props {
    /** Server being edited; null = new server. */
    initial: { name: string; def: McpServerDef } | null;
    /** Names already in use (excluding the edited one). */
    takenNames: string[];
    saving: boolean;
    /** Save error reported by the parent (shown inside the dialog). */
    error?: string;
    /** Returns false if saving failed (the error is shown via `error`). */
    onSubmit: (name: string, def: McpServerDef) => Promise<boolean>;
    onCancel: () => void;
  }

  let { initial, takenNames, saving, error = '', onSubmit, onCancel }: Props = $props();

  const uid = Math.random().toString(36).slice(2, 8);
  const id = (k: string) => `mcp-${k}-${uid}`;

  // Form fields are seeded once from `initial` (the form is re-created for each open).
  const init = untrack(() => initial);
  let name = $state(init?.name ?? '');
  let kind = $state<'stdio' | 'http'>(init?.def.url && !init?.def.command ? 'http' : 'stdio');
  let command = $state(init?.def.command ?? '');
  let argsText = $state((init?.def.args ?? []).join('\n'));
  let envText = $state(formatKeyValueLines(init?.def.env, '='));
  let url = $state(init?.def.url ?? '');
  let headersText = $state(formatKeyValueLines(init?.def.headers, ':'));
  const disabled = init?.def.disabled ?? false;
  let transport = $state<McpTransport>(init?.def.transport ?? 'auto');

  let submitted = $state(false);

  let nameError = $derived.by(() => {
    const n = name.trim();
    if (!n) return t('conn.mcp_err_name_required');
    if (!MCP_NAME_RE.test(n)) return t('conn.mcp_err_name_invalid');
    if (takenNames.includes(n)) return t('conn.mcp_err_name_taken');
    return '';
  });
  let commandError = $derived(kind === 'stdio' && !command.trim() ? t('conn.mcp_err_command_required') : '');
  let urlError = $derived(kind === 'http' && !/^https?:\/\/\S+$/.test(url.trim()) ? t('conn.mcp_err_url_invalid') : '');
  let envParsed = $derived(parseKeyValueLines(envText, '='));
  let headersParsed = $derived(parseKeyValueLines(headersText, ':'));
  let envError = $derived(kind === 'stdio' && !envParsed.ok ? t('conn.mcp_err_env_line', { line: envParsed.ok ? 0 : envParsed.line }) : '');
  let headersError = $derived(
    kind === 'http' && !headersParsed.ok ? t('conn.mcp_err_headers_line', { line: headersParsed.ok ? 0 : headersParsed.line }) : ''
  );
  let valid = $derived(!nameError && !commandError && !urlError && !envError && !headersError);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    submitted = true;
    if (!valid || saving) return;
    const def: McpServerDef = {};
    if (kind === 'stdio') {
      def.command = command.trim();
      const args = argsText.split(/\r?\n/).map((a) => a.trim()).filter(Boolean);
      if (args.length) def.args = args;
      if (envParsed.ok && Object.keys(envParsed.map).length) def.env = envParsed.map;
    } else {
      def.url = url.trim();
      if (transport !== 'auto') def.transport = transport;
      if (headersParsed.ok && Object.keys(headersParsed.map).length) def.headers = headersParsed.map;
    }
    if (disabled) def.disabled = true;
    await onSubmit(name.trim(), def);
  }
  const existingHelpId = $props.id();
</script>

<form class="flex min-w-0 flex-col gap-4" onsubmit={submit} novalidate>
  <Dialog.Header>
    <Dialog.Title>{initial ? t('conn.mcp_edit_title', { name: initial.name }) : t('conn.mcp_add_title')}</Dialog.Title>
  </Dialog.Header>

  <div class="flex max-h-[60vh] flex-col gap-3 overflow-y-auto pr-1">
    <Field.Field>
      <Field.Label for={id('name')}>{t('conn.mcp_field_name')}</Field.Label>
      <Input aria-describedby={existingHelpId + '-existing-conn-mcp_field_name_hint'}
        id={id('name')}
        class="h-8 font-mono"
        bind:value={name}
        placeholder="filesystem"
        autocomplete="off"
        spellcheck="false"
        aria-invalid={submitted && !!nameError}
      />
      {#if uiLevel.advanced}<HelpPopover text={t('conn.mcp_field_name_hint')} />{:else}<p class="text-xs text-muted-foreground">{t('conn.mcp_field_name_hint')}</p>{/if}<span id={existingHelpId + '-existing-conn-mcp_field_name_hint'} class="sr-only" data-field-help-text>{t('conn.mcp_field_name_hint')}</span>
      {#if submitted && nameError}<Field.Error>{nameError}</Field.Error>{/if}
    </Field.Field>

    <Field.Field>
      <Field.Label>{t('conn.mcp_field_type')}</Field.Label>
      <FieldHelp text={t('help.mcp_transport')}><ToggleGroup.Root
        type="single"
        variant="outline"
        size="sm"
        value={kind}
        onValueChange={(v) => {
          if (v === 'stdio' || v === 'http') kind = v;
        }}
        aria-label={t('conn.mcp_field_type')}
      >
        <ToggleGroup.Item value="stdio">{t('conn.mcp_type_stdio')}</ToggleGroup.Item>
        <ToggleGroup.Item value="http">{t('conn.mcp_type_http')}</ToggleGroup.Item>
      </ToggleGroup.Root></FieldHelp>
    </Field.Field>

    {#if kind === 'stdio'}
      <Field.Field>
        <Field.Label for={id('cmd')}>{t('conn.mcp_field_command')}</Field.Label>
        <Input aria-describedby={existingHelpId + '-existing-conn-mcp_field_command_hint'}
          id={id('cmd')}
          class="h-8 font-mono"
          bind:value={command}
          placeholder="npx"
          autocomplete="off"
          spellcheck="false"
          aria-invalid={submitted && !!commandError}
        />
        {#if uiLevel.advanced}<HelpPopover text={t('conn.mcp_field_command_hint')} />{:else}<p class="text-xs text-muted-foreground">{t('conn.mcp_field_command_hint')}</p>{/if}<span id={existingHelpId + '-existing-conn-mcp_field_command_hint'} class="sr-only" data-field-help-text>{t('conn.mcp_field_command_hint')}</span>
        <p class="flex items-start gap-1.5 text-xs text-warning"><ShieldAlert class="mt-0.5 size-3 shrink-0" /> {t('conn.mcp_stdio_notice')}</p>
        {#if submitted && commandError}<Field.Error>{commandError}</Field.Error>{/if}
      </Field.Field>
      <Field.Field>
        <Field.Label for={id('args')}>{t('conn.mcp_field_args')}</Field.Label>
        <Textarea aria-describedby={existingHelpId + '-existing-conn-mcp_field_args_hint'}
          id={id('args')}
          class="min-h-16 font-mono text-xs"
          rows={3}
          bind:value={argsText}
          placeholder={'-y\n@modelcontextprotocol/server-filesystem\nC:/Users/me/Documents'}
          spellcheck="false"
        />
        {#if uiLevel.advanced}<HelpPopover text={t('conn.mcp_field_args_hint')} />{:else}<p class="text-xs text-muted-foreground">{t('conn.mcp_field_args_hint')}</p>{/if}<span id={existingHelpId + '-existing-conn-mcp_field_args_hint'} class="sr-only" data-field-help-text>{t('conn.mcp_field_args_hint')}</span>
      </Field.Field>
      <Field.Field>
        <Field.Label for={id('env')}>{t('conn.mcp_field_env')}</Field.Label>
        <Textarea aria-describedby={existingHelpId + '-existing-conn-mcp_field_env_hint'}
          id={id('env')}
          class="min-h-14 font-mono text-xs"
          rows={2}
          bind:value={envText}
          placeholder="API_KEY=..."
          spellcheck="false"
          aria-invalid={submitted && !!envError}
        />
        {#if uiLevel.advanced}<HelpPopover text={t('conn.mcp_field_env_hint')} />{:else}<p class="text-xs text-muted-foreground">{t('conn.mcp_field_env_hint')}</p>{/if}<span id={existingHelpId + '-existing-conn-mcp_field_env_hint'} class="sr-only" data-field-help-text>{t('conn.mcp_field_env_hint')}</span>
        {#if submitted && envError}<Field.Error>{envError}</Field.Error>{/if}
      </Field.Field>
    {:else}
      <Field.Field>
        <Field.Label for={id('url')}>{t('conn.mcp_field_url')}</Field.Label>
        <FieldHelp text={t('help.mcp_url')}><Input
          id={id('url')}
          class="h-8 font-mono"
          type="url"
          bind:value={url}
          placeholder="https://example.com/mcp"
          autocomplete="off"
          spellcheck="false"
          aria-invalid={submitted && !!urlError}
        /></FieldHelp>
        {#if submitted && urlError}<Field.Error>{urlError}</Field.Error>{/if}
      </Field.Field>
      <Field.Field>
        <Field.Label for={id('transport')}>{t('conn.mcp_field_transport')}</Field.Label>
        <FieldHelp text={t('conn.mcp_transport_hint')}><NativeSelect
          id={id('transport')}
          class="h-8 text-sm"
          value={transport}
          onchange={(e) => (transport = e.currentTarget.value as McpTransport)}
        >
          {#each MCP_TRANSPORTS as tr (tr)}
            <option value={tr}>{t(`conn.mcp_transport_${tr}` as TKey)}</option>
          {/each}
        </NativeSelect></FieldHelp>
      </Field.Field>
      <Field.Field>
        <Field.Label for={id('headers')}>{t('conn.mcp_field_headers')}</Field.Label>
        <Textarea aria-describedby={existingHelpId + '-existing-conn-mcp_field_headers_hint'}
          id={id('headers')}
          class="min-h-14 font-mono text-xs"
          rows={2}
          bind:value={headersText}
          placeholder="Authorization: Bearer ..."
          spellcheck="false"
          aria-invalid={submitted && !!headersError}
        />
        {#if uiLevel.advanced}<HelpPopover text={t('conn.mcp_field_headers_hint')} />{:else}<p class="text-xs text-muted-foreground">{t('conn.mcp_field_headers_hint')}</p>{/if}<span id={existingHelpId + '-existing-conn-mcp_field_headers_hint'} class="sr-only" data-field-help-text>{t('conn.mcp_field_headers_hint')}</span>
        {#if submitted && headersError}<Field.Error>{headersError}</Field.Error>{/if}
      </Field.Field>
    {/if}
  </div>

  {#if error}
    <p class="text-xs whitespace-pre-line text-destructive" role="alert">{error}</p>
  {/if}

  <Dialog.Footer>
    <Button type="button" variant="outline" onclick={onCancel}>{t('conn.mcp_cancel')}</Button>
    <Button type="submit" disabled={saving}>{saving ? t('conn.mcp_saving') : t('conn.mcp_save')}</Button>
  </Dialog.Footer>
</form>
