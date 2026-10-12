<script lang="ts">
  import Field from '../dashboard/parts/Field.svelte';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  // Session panel section: context compaction settings, either the global defaults or this
  // conversation's own (PgSettings.compaction).
  import { t } from '#lib/i18n.svelte.js';
  import { modelLabel } from '#lib/format.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Switch } from '#lib/components/ui/switch/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import * as UiField from '#lib/components/ui/field/index.js';
  import * as ToggleGroup from '#lib/components/ui/toggle-group/index.js';
  import InfoTip from '../common/InfoTip.svelte';
  import NativeSelect from '../dashboard/parts/NativeSelect.svelte';
  import {
    KEEP_MAX,
    PERCENT_MAX,
    PERCENT_MIN,
    SUMMARY_TOKENS_MAX,
    TOKENS_MIN,
    compactionDefaults,
    sanitizeCompaction,
    saveGlobalCompaction,
    thresholdTokens,
    type CompactionSettings,
  } from './compaction.svelte';
  import type { PgSettings } from './types';

  interface Props {
    settings: PgSettings;
    /** Models the summary can be written by (the gateway's list, else the server's). */
    models: string[];
    /** The chat model's context in tokens, when known. */
    contextSize: number | null;
  }

  let { settings = $bindable(), models, contextSize }: Props = $props();

  const uid = $props.id();
  const id = (name: string) => `cmp-${name}-${uid}`;

  let own = $derived(settings.compaction !== null);
  let cur = $derived(settings.compaction ?? compactionDefaults.value);

  function update(patch: Partial<CompactionSettings>) {
    const next = sanitizeCompaction({ ...cur, ...patch });
    if (settings.compaction) settings.compaction = next;
    else saveGlobalCompaction(next);
  }

  function setOwn(on: boolean) {
    settings.compaction = on ? { ...compactionDefaults.value } : null;
  }

  /** Commits a number field on change (blur / Enter), so typing is not clamped mid-way. */
  function num(e: Event, key: 'percent' | 'tokens' | 'keepTurns') {
    const el = e.currentTarget as HTMLInputElement;
    const v = el.valueAsNumber;
    const next = sanitizeCompaction({ ...cur, [key]: Number.isNaN(v) ? cur[key] : v });
    update(next);
    // Show the clamped value (the bound value may not change).
    el.value = String(next[key]);
  }

  let limit = $derived(contextSize ? thresholdTokens(cur, contextSize) : null);
</script>

{#snippet hint(text: string)}
  <InfoTip {text} />
{/snippet}

<UiField.Set class="gap-3">
  <div class="flex items-center gap-1.5">
    <UiField.Legend variant="label" class="mb-0">{t('cmp.title')}</UiField.Legend>
    {@render hint(t('cmp.title_hint'))}
  </div>

  <Field label={t('cmp.own')} id={id('own')} help={t('cmp.own_hint')} note={own ? t('cmp.scope_own') : t('cmp.scope_global')}>
    <Switch id={id('own')} checked={own} onCheckedChange={setOwn} size="sm" />
  </Field>

  <UiField.Field orientation="horizontal" class="items-center justify-between">
    <UiField.Label for={id('auto')} class="text-sm font-medium">{t('cmp.auto')}</UiField.Label>
    <FieldHelp text={t('help.cmp_auto')}><Switch id={id('auto')} checked={cur.auto} onCheckedChange={(v) => update({ auto: v })} size="sm" /></FieldHelp>
  </UiField.Field>

  <Field label={t('cmp.threshold')} id={id('threshold')} hint={t('cmp.threshold_hint')} note={limit !== null && contextSize ? t('cmp.threshold_now', { limit: limit.toLocaleString(), ctx: contextSize.toLocaleString() }) + (cur.mode === 'tokens' && cur.tokens > limit ? ' ' + t('cmp.threshold_capped') : '') : undefined}>
    <div class="flex items-center justify-between gap-2">
      <ToggleGroup.Root
        type="single"
        variant="outline"
        size="sm"
        value={cur.mode}
        onValueChange={(v) => {
          if (v) update({ mode: v as CompactionSettings['mode'] });
        }}
        aria-label={t('cmp.mode_label')}
      >
        <ToggleGroup.Item value="percent" class="h-6 px-2 text-xs">{t('cmp.mode_percent')}</ToggleGroup.Item>
        <ToggleGroup.Item value="tokens" class="h-6 px-2 text-xs">{t('cmp.mode_tokens')}</ToggleGroup.Item>
      </ToggleGroup.Root>
    </div>
    <div class="flex items-center gap-2">
      {#if cur.mode === 'percent'}
        <Input
          id={id('threshold')}
          type="number"
          min={PERCENT_MIN}
          max={PERCENT_MAX}
          class="h-8 w-24 text-right font-mono tabular"
          value={cur.percent}
          onchange={(e) => num(e, 'percent')}
        />
        <span class="text-xs text-muted-foreground">%</span>
      {:else}
        <Input
          id={id('threshold')}
          type="number"
          min={TOKENS_MIN}
          step="500"
          class="h-8 w-28 text-right font-mono tabular"
          value={cur.tokens}
          onchange={(e) => num(e, 'tokens')}
        />
        <span class="text-xs text-muted-foreground">{t('cmp.tokens_unit')}</span>
      {/if}
    </div>

  </Field>

  <Field label={t('cmp.keep')} id={id('keep')} note={t('cmp.keep_hint')}>
    <Input
      id={id('keep')} aria-describedby={id('keep') + '-help'}
      type="number"
      min="0"
      max={KEEP_MAX}
      class="h-8 w-20 text-right font-mono tabular"
      value={cur.keepTurns}
      onchange={(e) => num(e, 'keepTurns')}
    />
  </Field>

  <Field label={t('cmp.model')} id={id('model')} note={t('cmp.model_hint')}>
    <NativeSelect
      id={id('model')} aria-describedby={id('model') + '-help'}
      class="text-xs"
      value={cur.summaryModel}
      onchange={(e: Event) => update({ summaryModel: (e.currentTarget as HTMLSelectElement).value })}
    >
      <option value="">{t('cmp.model_chat')}</option>
      {#if cur.summaryModel && !models.includes(cur.summaryModel)}
        <option value={cur.summaryModel}>{modelLabel(cur.summaryModel)} ({t('cmp.model_missing')})</option>
      {/if}
      {#each models as m (m)}
        <option value={m}>{modelLabel(m)}</option>
      {/each}
    </NativeSelect>
  </Field>

  <Field label={t('cmp.clear_tools')} id={id('tools')} note={t('cmp.clear_tools_hint')}>
    <Switch id={id('tools')} aria-describedby={id('tools') + '-help'} checked={cur.clearToolResults} onCheckedChange={(v) => update({ clearToolResults: v })} size="sm" />
  </Field>

  <Field label={t('cmp.summary_size')} id={id('size')} help={t('cmp.summary_size_hint')}>
    <Input
      id={id('size')} aria-describedby={id('size') + '-help'}
      type="number"
      min="0"
      max={SUMMARY_TOKENS_MAX}
      step="64"
      placeholder={t('cmp.auto_value')}
      class="h-8 w-24 text-right font-mono tabular"
      value={cur.summaryTokens || ''}
      onchange={(e) => {
        const el = e.currentTarget as HTMLInputElement;
        update({ summaryTokens: el.value.trim() === '' || Number.isNaN(el.valueAsNumber) ? 0 : el.valueAsNumber });
      }}
    />
  </Field>

  <UiField.Field class="gap-1.5">
    <UiField.Label for={id('instruction')} class="text-sm font-medium">{t('cmp.instruction')}</UiField.Label>
    <FieldHelp text={t('help.cmp_instruction')}><Textarea
      id={id('instruction')}
      rows={2}
      class="min-h-12 resize-y text-xs"
      placeholder={t('cmp.instruction_placeholder')}
      value={cur.instruction}
      onchange={(e) => update({ instruction: (e.currentTarget as HTMLTextAreaElement).value })}
    /></FieldHelp>
  </UiField.Field>
</UiField.Set>
