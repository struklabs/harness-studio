<script lang="ts">
  import Field from '../dashboard/parts/Field.svelte';
  import FieldHelp from '$lib/components/common/FieldHelp.svelte';
  // Editor for the request-side settings of an inference preset (sampling, reasoning,
  // output format). Used by the preset manager and the Playground settings panel.
  import type { SamplingParams } from '$lib/types';
  import { t, type TKey } from '$lib/i18n.svelte';
  import { cn } from '$lib/utils';
  import { capsStore } from '$lib/stores/caps.svelte';
  import { Input } from '$lib/components/ui/input';
  import { Textarea } from '$lib/components/ui/textarea';
  import { Button } from '$lib/components/ui/button';
  import * as Collapsible from '$lib/components/ui/collapsible';
  import NativeSelect from '$lib/components/dashboard/parts/NativeSelect.svelte';
  import { ChevronRight } from '@lucide/svelte';
  import type { PresetRequest } from './request';

  interface Props {
    page?: 'sampling' | 'output';
    request: PresetRequest;
    disabled?: boolean;
    /** Narrow layout (Playground side panel). */
    compact?: boolean;
  }

  let { page, request = $bindable(), disabled = false, compact = false }: Props = $props();

  const uid = $props.id();
  const id = (name: string) => `rq-${name}-${uid}`;
  let advancedOpen = $state(false);
  $effect(() => { if (page) advancedOpen = true; });

  type NumKey = {
    [K in keyof SamplingParams]-?: SamplingParams[K] extends number | undefined ? K : never;
  }[keyof SamplingParams];

  interface NumField {
    key: NumKey;
    label: TKey;
    placeholder: string;
    step: number;
    min?: number;
    max?: number;
    /** Runtime flags that show the sampler exists (hidden when the runtime lacks all). */
    flags?: string[];
  }

  const BASIC: NumField[] = [
    { key: 'temperature', label: 'ip.f_temperature', placeholder: '0.8', step: 0.05, min: 0, max: 5 },
    { key: 'top_p', label: 'ip.f_top_p', placeholder: '0.95', step: 0.01, min: 0, max: 1 },
    { key: 'max_tokens', label: 'ip.f_max_tokens', placeholder: '∞', step: 1, min: -1 },
  ];

  const GROUPS: { title: TKey; fields: NumField[] }[] = [
    {
      title: 'ip.g_sampling',
      fields: [
        { key: 'min_p', label: 'ip.f_min_p', placeholder: '0.05', step: 0.01, min: 0, max: 1, flags: ['--min-p'] },
        { key: 'top_k', label: 'ip.f_top_k', placeholder: '40', step: 1, min: 0 },
        { key: 'typical_p', label: 'ip.f_typical_p', placeholder: '1.0', step: 0.01, min: 0, max: 1, flags: ['--typical', '--typical-p'] },
        { key: 'top_n_sigma', label: 'ip.f_top_n_sigma', placeholder: '-1', step: 0.1, flags: ['--top-nsigma', '--top-n-sigma'] },
        { key: 'seed', label: 'ip.f_seed', placeholder: t('ip.placeholder_random'), step: 1 },
      ],
    },
    {
      title: 'ip.g_penalties',
      fields: [
        { key: 'repeat_penalty', label: 'ip.f_repeat_penalty', placeholder: '1.0', step: 0.05, min: 0 },
        { key: 'repeat_last_n', label: 'ip.f_repeat_last_n', placeholder: '64', step: 1, min: -1, flags: ['--repeat-last-n'] },
        { key: 'presence_penalty', label: 'ip.f_presence_penalty', placeholder: '0', step: 0.05, min: -2, max: 2, flags: ['--presence-penalty'] },
        { key: 'frequency_penalty', label: 'ip.f_frequency_penalty', placeholder: '0', step: 0.05, min: -2, max: 2, flags: ['--frequency-penalty'] },
      ],
    },
    {
      title: 'ip.g_dry',
      fields: [
        { key: 'dry_multiplier', label: 'ip.f_dry_multiplier', placeholder: '0', step: 0.05, min: 0, flags: ['--dry-multiplier'] },
        { key: 'dry_base', label: 'ip.f_dry_base', placeholder: '1.75', step: 0.05, min: 0, flags: ['--dry-multiplier'] },
        { key: 'dry_allowed_length', label: 'ip.f_dry_allowed_length', placeholder: '2', step: 1, min: 0, flags: ['--dry-multiplier'] },
        { key: 'dry_penalty_last_n', label: 'ip.f_dry_penalty_last_n', placeholder: '-1', step: 1, min: -1, flags: ['--dry-multiplier'] },
      ],
    },
    {
      title: 'ip.g_xtc',
      fields: [
        { key: 'xtc_probability', label: 'ip.f_xtc_probability', placeholder: '0', step: 0.05, min: 0, max: 1, flags: ['--xtc-probability'] },
        { key: 'xtc_threshold', label: 'ip.f_xtc_threshold', placeholder: '0.1', step: 0.01, min: 0, max: 1, flags: ['--xtc-probability'] },
      ],
    },
    {
      title: 'ip.g_dynatemp',
      fields: [
        { key: 'dynatemp_range', label: 'ip.f_dynatemp_range', placeholder: '0', step: 0.05, min: 0, max: 5, flags: ['--dynatemp-range'] },
        { key: 'dynatemp_exponent', label: 'ip.f_dynatemp_exponent', placeholder: '1.0', step: 0.05, min: 0, max: 10, flags: ['--dynatemp-range'] },
      ],
    },
    {
      title: 'ip.g_adaptive',
      fields: [
        { key: 'adaptive_target', label: 'ip.f_adaptive_target', placeholder: '-1', step: 0.05, min: -1, max: 1, flags: ['--adaptive-target'] },
        { key: 'adaptive_decay', label: 'ip.f_adaptive_decay', placeholder: '0.9', step: 0.01, min: 0, max: 0.99, flags: ['--adaptive-target'] },
      ],
    },
    {
      title: 'ip.g_mirostat',
      fields: [
        { key: 'mirostat_tau', label: 'ip.f_mirostat_tau', placeholder: '5.0', step: 0.1, min: 0, flags: ['--mirostat'] },
        { key: 'mirostat_eta', label: 'ip.f_mirostat_eta', placeholder: '0.1', step: 0.01, min: 0, max: 1, flags: ['--mirostat'] },
      ],
    },
  ];

  const supported = (flags?: string[]) => !flags || capsStore.hasAny(flags);
  let hiddenCount = $derived(
    GROUPS.flatMap((g) => g.fields).filter((f) => !supported(f.flags)).length +
      (supported(['--mirostat']) ? 0 : 1) +
      (supported(['--reasoning-format']) ? 0 : 1) +
      (supported(['--reasoning-budget']) ? 0 : 1)
  );
  let advancedSet = $derived(
    GROUPS.flatMap((g) => g.fields).filter((f) => request.sampling[f.key] !== undefined).length +
      ['samplers', 'logit_bias', 'grammar', 'json_schema', 'dry_sequence_breakers', 'mirostat', 'logprobs', 'top_logprobs'].filter(
        (k) => (request.sampling as Record<string, unknown>)[k] !== undefined
      ).length +
      (request.response_format !== undefined ? 1 : 0) +
      (request.tool_choice !== undefined ? 1 : 0)
  );

  function setNum(key: NumKey, raw: string, int: boolean) {
    const s = request.sampling as Record<string, unknown>;
    if (raw.trim() === '') {
      delete s[key];
      return;
    }
    const n = Number(raw);
    if (!Number.isFinite(n)) return;
    s[key] = int ? Math.trunc(n) : n;
  }

  const INT = new Set<NumKey>(['top_k', 'repeat_last_n', 'dry_allowed_length', 'dry_penalty_last_n', 'seed', 'max_tokens', 'top_logprobs']);

  // ---- lists (one item per line) ----
  function listText(v: string[] | undefined): string {
    return (v ?? []).map((x) => x.replace(/\n/g, '\\n')).join('\n');
  }
  function setList(key: 'stop' | 'dry_sequence_breakers', raw: string) {
    const items = raw
      .split('\n')
      .map((x) => x.replace(/\\n/g, '\n'))
      .filter((x) => x !== '');
    if (items.length) request.sampling[key] = items;
    else delete request.sampling[key];
  }

  function setSamplers(raw: string) {
    const items = raw
      .split(/[,;\s]+/)
      .map((x) => x.trim())
      .filter(Boolean);
    if (items.length) request.sampling.samplers = items;
    else delete request.sampling.samplers;
  }

  // ---- JSON fields: edited as text, applied when valid ----
  type JsonKey = 'logit_bias' | 'json_schema' | 'response_format' | 'tool_choice';
  let jsonText = $state<Record<JsonKey, string>>({ logit_bias: '', json_schema: '', response_format: '', tool_choice: '' });
  let jsonError = $state<Partial<Record<JsonKey, string>>>({});

  function currentJson(key: JsonKey): unknown {
    if (key === 'response_format' || key === 'tool_choice') return request[key];
    return request.sampling[key];
  }

  // Refresh the texts when the request object is replaced (preset applied, reset).
  $effect(() => {
    for (const key of ['logit_bias', 'json_schema', 'response_format', 'tool_choice'] as JsonKey[]) {
      const v = currentJson(key);
      const text = v === undefined || v === null ? '' : key === 'tool_choice' && typeof v === 'string' ? v : JSON.stringify(v, null, 2);
      let same = false;
      try {
        same = jsonText[key].trim() !== '' && JSON.stringify(parseLoose(key, jsonText[key])) === JSON.stringify(v);
      } catch {
        same = false;
      }
      if (!same && !jsonError[key]) jsonText[key] = text;
    }
  });

  function parseLoose(key: JsonKey, raw: string): unknown {
    const s = raw.trim();
    if (s === '') return undefined;
    if (key === 'tool_choice' && ['auto', 'none', 'required'].includes(s)) return s;
    return JSON.parse(s);
  }

  function applyJson(key: JsonKey) {
    let v: unknown;
    try {
      v = parseLoose(key, jsonText[key]);
    } catch (e) {
      jsonError[key] = t('ip.json_invalid', { message: e instanceof Error ? e.message : String(e) });
      return;
    }
    const objOnly = key === 'json_schema' || key === 'response_format';
    if (v !== undefined && objOnly && (typeof v !== 'object' || Array.isArray(v) || v === null)) {
      jsonError[key] = t('ip.json_object_required');
      return;
    }
    jsonError[key] = undefined;
    if (key === 'response_format' || key === 'tool_choice') {
      if (v === undefined) delete request[key];
      else request[key] = v;
    } else if (v === undefined) {
      delete request.sampling[key];
    } else {
      request.sampling[key] = v;
    }
  }

  function setReasoning<K extends keyof PresetRequest['reasoning']>(key: K, v: PresetRequest['reasoning'][K] | undefined) {
    if (v === undefined) delete request.reasoning[key];
    else request.reasoning[key] = v;
  }
</script>

{#snippet numField(f: NumField)}
  {@const v = request.sampling[f.key]}
  <Field label={t(f.label)} id={id(f.key)} hint={t(('help.sample_' + f.key) as TKey)}><Input
      id={id(f.key)}
      type="number"
      step={f.step}
      min={f.min}
      max={f.max}
      {disabled}
      class="h-7 font-num text-xs"
      placeholder={f.placeholder}
      value={v ?? ''}
      oninput={(e) => setNum(f.key, (e.currentTarget as HTMLInputElement).value, INT.has(f.key))}
    />
  </Field>
{/snippet}

{#snippet jsonField(key: JsonKey, label: TKey, placeholder: string, rows = 3)}
  <Field label={t(label)} id={id(key)} help={t(('help.json_' + key) as TKey)} note={key === 'tool_choice' ? t('ip.tool_choice_hint') : undefined}><Textarea
      id={id(key)}
      {rows}
      {disabled}
      class={cn('min-h-12 resize-y font-mono text-xs', jsonError[key] && 'border-destructive')}
      {placeholder}
      bind:value={jsonText[key]}
      onblur={() => applyJson(key)}
      aria-invalid={jsonError[key] ? 'true' : undefined}
    />
    {#if jsonError[key]}<p class="text-xs text-destructive" role="alert">{jsonError[key]}</p>{/if}
  </Field>
{/snippet}

<div class="flex min-w-0 flex-col gap-3">
  <p class="text-xs text-muted-foreground" role="note">{t('help.sample_blank')}</p>
  <div class={cn('grid gap-2', compact ? 'grid-cols-[repeat(3,minmax(0,1fr))]' : 'grid-cols-[repeat(3,minmax(0,1fr))]')}>
    {#each BASIC.filter(f => !page || (page === 'output' ? f.key === 'max_tokens' : f.key !== 'max_tokens')) as f (f.key)}
      {@render numField(f)}
    {/each}
  </div>

<div hidden={page === 'sampling'} class="flex flex-col gap-3">
  <div class="flex min-w-0 flex-col gap-1">
    <label for={id('stop')} class="text-sm font-medium text-foreground">{t('ip.f_stop')}</label>
    <FieldHelp text={t('help.stop')}><Textarea
      id={id('stop')}
      rows={2}
      {disabled}
      class="min-h-10 resize-y font-mono text-xs"
      placeholder={t('ip.stop_placeholder')}
      value={listText(request.sampling.stop)}
      onchange={(e) => setList('stop', (e.currentTarget as HTMLTextAreaElement).value)}
    /></FieldHelp>
  </div>

</div>
<div hidden={page === 'output'} class="flex flex-col gap-3">
  <!-- Reasoning -->
  <div class={cn('grid gap-2', compact ? 'grid-cols-[repeat(2,minmax(0,1fr))]' : 'grid-cols-[repeat(4,minmax(0,1fr))]')}>
    <div class="flex min-w-0 flex-col gap-1">
      <label for={id('think')} class="truncate text-sm font-medium text-foreground">{t('ip.f_thinking')}</label>
      <FieldHelp text={t('param.reasoning_help')}><NativeSelect
        id={id('think')}
        {disabled}
        value={request.reasoning.enabled === undefined ? '' : request.reasoning.enabled ? 'on' : 'off'}
        onchange={(e: Event) => {
          const v = (e.currentTarget as HTMLSelectElement).value;
          setReasoning('enabled', v === '' ? undefined : v === 'on');
        }}
      >
        <option value="">{t('ip.opt_model_default')}</option>
        <option value="on">{t('ip.opt_on')}</option>
        <option value="off">{t('ip.opt_off')}</option>
      </NativeSelect></FieldHelp>
    </div>
    {#if supported(['--reasoning-format'])}
      <div class="flex min-w-0 flex-col gap-1">
        <label for={id('rfmt')} class="truncate text-sm font-medium text-foreground">{t('ip.f_reasoning_format')}</label>
        <FieldHelp text={t('param.reasoning_format_help')}><NativeSelect
          id={id('rfmt')}
          {disabled}
          value={request.reasoning.format ?? ''}
          onchange={(e: Event) => setReasoning('format', (e.currentTarget as HTMLSelectElement).value || undefined)}
        >
          <option value="">{t('ip.opt_server_default')}</option>
          {#each ['auto', 'none', 'deepseek', 'deepseek-legacy'] as f (f)}
            <option value={f}>{f}</option>
          {/each}
        </NativeSelect></FieldHelp>
      </div>
    {/if}
    <div class="flex min-w-0 flex-col gap-1">
      <label for={id('effort')} class="truncate text-sm font-medium text-foreground">{t('ip.f_reasoning_effort')}</label>
      <FieldHelp text={t('param.reasoning_effort_help')}><NativeSelect
        id={id('effort')}
        {disabled}
        value={request.reasoning.effort ?? ''}
        onchange={(e: Event) => setReasoning('effort', (e.currentTarget as HTMLSelectElement).value || undefined)}
      >
        <option value="">{t('ip.opt_model_default')}</option>
        {#each ['low', 'medium', 'high'] as f (f)}
          <option value={f}>{f}</option>
        {/each}
      </NativeSelect></FieldHelp>
    </div>
    {#if supported(['--reasoning-budget'])}
      <div class="flex min-w-0 flex-col gap-1">
        <label for={id('budget')} class="truncate text-sm font-medium text-foreground">{t('ip.f_reasoning_budget')}</label>
        <FieldHelp text={t('param.reasoning_budget_help')}><Input
          id={id('budget')}
          type="number"
          step="1"
          min="-1"
          {disabled}
          class="h-7 font-num text-xs"
          placeholder="-1"
          value={request.reasoning.budget ?? ''}
          oninput={(e) => {
            const raw = (e.currentTarget as HTMLInputElement).value;
            const n = Number(raw);
            setReasoning('budget', raw.trim() === '' || !Number.isFinite(n) ? undefined : Math.trunc(n));
          }}
        /></FieldHelp>
      </div>
    {/if}
  </div>

</div>
  <Collapsible.Root bind:open={advancedOpen}>
    <Collapsible.Trigger
      class="group flex items-center gap-1 rounded-sm text-xs font-medium text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
    >
      <ChevronRight class="size-3.5 transition-transform group-data-[state=open]:rotate-90" aria-hidden="true" />
      {t('ip.advanced')}
      {#if advancedSet > 0}
        <span class="font-num text-xs text-muted-foreground">· {advancedSet}</span>
      {/if}
    </Collapsible.Trigger>
    <Collapsible.Content>
      <div class="flex flex-col gap-4 pt-3">
<div hidden={page === 'output'} class="flex flex-col gap-3">
        {#each GROUPS as g (g.title)}
          {@const fields = g.fields.filter((f) => supported(f.flags))}
          {#if fields.length}
            <div class="flex flex-col gap-2">
              {#if page !== 'sampling' || g.title !== 'ip.g_sampling'}
                <span class="text-sm font-semibold text-foreground">{t(g.title)}</span>
              {/if}
              <div class={cn('grid gap-2', compact ? 'grid-cols-[repeat(2,minmax(0,1fr))]' : 'grid-cols-[repeat(4,minmax(0,1fr))]')}>
                {#each fields as f (f.key)}
                  {@render numField(f)}
                {/each}
                {#if g.title === 'ip.g_mirostat'}
                  <div class="flex min-w-0 flex-col gap-1">
                    <label for={id('miro')} class="truncate text-sm font-medium text-foreground">{t('ip.f_mirostat')}</label>
                    <FieldHelp text={t('help.mirostat')}><NativeSelect
                      id={id('miro')}
                      {disabled}
                      value={request.sampling.mirostat === undefined ? '' : String(request.sampling.mirostat)}
                      onchange={(e: Event) => {
                        const v = (e.currentTarget as HTMLSelectElement).value;
                        if (v === '') delete request.sampling.mirostat;
                        else request.sampling.mirostat = Number(v);
                      }}
                    >
                      <option value="">{t('ip.opt_server_default')}</option>
                      <option value="0">0 · {t('ip.opt_off')}</option>
                      <option value="1">1 · Mirostat</option>
                      <option value="2">2 · Mirostat 2.0</option>
                    </NativeSelect></FieldHelp>
                  </div>
                {/if}
              </div>
              {#if g.title === 'ip.g_dry'}
                <div class="flex min-w-0 flex-col gap-1">
                  <label for={id('drybreak')} class="text-sm font-medium text-foreground">{t('ip.f_dry_sequence_breakers')}</label>
                  <FieldHelp text={t('help.dry_break')}><Textarea
                    id={id('drybreak')}
                    rows={2}
                    {disabled}
                    class="min-h-10 resize-y font-mono text-xs"
                    placeholder={'\\n\n:\n"\n*'}
                    value={listText(request.sampling.dry_sequence_breakers)}
                    onchange={(e) => setList('dry_sequence_breakers', (e.currentTarget as HTMLTextAreaElement).value)}
                  /></FieldHelp>
                </div>
              {/if}
            </div>
          {/if}
        {/each}

</div>
<div hidden={page === 'sampling'} class="flex flex-col gap-3">
        <div class="flex flex-col gap-2">
          <span class="text-sm font-semibold text-foreground">{t('ip.g_output')}</span>
          {#if supported(['--samplers'])}
            <div class="flex min-w-0 flex-col gap-1">
              <label for={id('samplers')} class="text-sm font-medium text-foreground">{t('ip.f_samplers')}</label>
              <FieldHelp text={t('help.samplers')}><Input
                id={id('samplers')}
                {disabled}
                class="h-7 font-mono text-xs"
                placeholder="penalties, dry, top_n_sigma, top_k, typ_p, top_p, min_p, xtc, temperature"
                value={(request.sampling.samplers ?? []).join(', ')}
                onchange={(e) => setSamplers((e.currentTarget as HTMLInputElement).value)}
              /></FieldHelp>
            </div>
          {/if}
          {@render jsonField('logit_bias', 'ip.f_logit_bias', '{"15043": -1.5}', 2)}
          <div class="flex min-w-0 flex-col gap-1">
            <label for={id('grammar')} class="text-sm font-medium text-foreground">{t('ip.f_grammar')}</label>
            <FieldHelp text={t('help.grammar')}><Textarea
              id={id('grammar')}
              rows={3}
              {disabled}
              class="min-h-12 resize-y font-mono text-xs"
              placeholder={'root ::= "yes" | "no"'}
              value={request.sampling.grammar ?? ''}
              onchange={(e) => {
                const v = (e.currentTarget as HTMLTextAreaElement).value;
                if (v.trim()) request.sampling.grammar = v;
                else delete request.sampling.grammar;
              }}
            /></FieldHelp>
          </div>
          {@render jsonField('json_schema', 'ip.f_json_schema', '{"type": "object", "properties": {…}}')}
          {@render jsonField('response_format', 'ip.f_response_format', '{"type": "json_object"}', 2)}
          {@render jsonField('tool_choice', 'ip.f_tool_choice', 'auto | none | required | {"type": "function", …}', 1)}
          <div class={cn('grid gap-2', compact ? 'grid-cols-[repeat(2,minmax(0,1fr))]' : 'grid-cols-[repeat(4,minmax(0,1fr))]')}>
            <div class="flex min-w-0 flex-col gap-1">
              <label for={id('logprobs')} class="truncate text-sm font-medium text-foreground">{t('ip.f_logprobs')}</label>
              <FieldHelp text={t('help.sample_logprobs')}><NativeSelect
                id={id('logprobs')}
                {disabled}
                value={request.sampling.logprobs === undefined ? '' : request.sampling.logprobs ? 'on' : 'off'}
                onchange={(e: Event) => {
                  const v = (e.currentTarget as HTMLSelectElement).value;
                  if (v === '') delete request.sampling.logprobs;
                  else request.sampling.logprobs = v === 'on';
                }}
              >
                <option value="">{t('ip.opt_server_default')}</option>
                <option value="on">{t('ip.opt_on')}</option>
                <option value="off">{t('ip.opt_off')}</option>
              </NativeSelect></FieldHelp>
            </div>
            {@render numField({ key: 'top_logprobs', label: 'ip.f_top_logprobs', placeholder: '5', step: 1, min: 0, max: 20 })}
          </div>
        </div>

</div>
        {#if hiddenCount > 0}
          <p class="text-xs text-muted-foreground" role="note">{t('ip.hidden_unsupported', { count: hiddenCount })}</p>
        {/if}
        {#if advancedSet > 0}
          <Button
            variant="ghost"
            size="xs"
            class="self-start text-xs"
            {disabled}
            onclick={() => {
              const keep = {
                temperature: request.sampling.temperature,
                top_p: request.sampling.top_p,
                max_tokens: request.sampling.max_tokens,
                stop: request.sampling.stop,
              };
              request.sampling = Object.fromEntries(Object.entries(keep).filter(([, v]) => v !== undefined));
              delete request.response_format;
              delete request.tool_choice;
              jsonError = {};
            }}
          >
            {t('ip.clear_advanced')}
          </Button>
        {/if}
      </div>
    </Collapsible.Content>
  </Collapsible.Root>
</div>
