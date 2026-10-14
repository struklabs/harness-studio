<script lang="ts">
  import AdvancedOnly from '#lib/components/common/AdvancedOnly.svelte';
  import type { ParamSpec, ParamValue } from '#lib/types.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { capsStore } from '#lib/stores/caps.svelte.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import { Switch } from '#lib/components/ui/switch/index.js';
  import { Checkbox } from '#lib/components/ui/checkbox/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { isUnsetParam, type LaunchParamsForm } from '../params';
  import Field from '../parts/Field.svelte';
  import NativeSelect from '../parts/NativeSelect.svelte';
  import FeatureBadge from './FeatureBadge.svelte';
  import MetadataOverrides from './MetadataOverrides.svelte';

  interface Props {
    params: LaunchParamsForm;
    /** Catalog group (params.toml `group`). */
    group: string;
    exclude?: string[];
  }

  let { params = $bindable(), group, exclude = [] }: Props = $props();

  const uid = $props.id();
  let showAll = $state(false);

  let all = $derived(capsStore.groupSpecs(group).filter((s) => !exclude.includes(s.id)));
  /** Only parameters this runtime has (all of them while the runtime is unknown). */
  let supported = $derived(all.filter((s) => capsStore.hasAny(s.flags) || hasFallback(s)));
  let hiddenCount = $derived(all.length - supported.length);
  let visible = $derived(
    supported.filter((s) => showAll || s.level === 'basic' || !isUnsetParam(params.catalog_params[s.id]))
  );
  let advancedCount = $derived(supported.filter((s) => s.level === 'advanced').length);

  /** Older runtimes: values mapped to legacy flags (e.g. load mode -> --no-mmap). */
  function hasFallback(spec: ParamSpec): boolean {
    return spec.id === 'load_mode' && capsStore.hasAny(['--no-mmap', '--mlock']);
  }

  function value(id: string): ParamValue | undefined {
    return params.catalog_params[id];
  }

  function set(id: string, v: ParamValue | undefined) {
    if (v === undefined || isUnsetParam(v)) delete params.catalog_params[id];
    else params.catalog_params[id] = v;
  }

  function flagLabel(spec: ParamSpec): string {
    return spec.flags.find((f) => capsStore.flag(f)) ?? spec.flags[0];
  }

  function num(spec: ParamSpec, raw: string) {
    if (raw.trim() === '') return set(spec.id, undefined);
    const n = Number(raw);
    set(spec.id, Number.isFinite(n) ? n : raw);
  }

  function toggleMulti(spec: ParamSpec, item: string, on: boolean) {
    const cur = value(spec.id);
    const list = Array.isArray(cur) ? cur : typeof cur === 'string' && cur ? cur.split(',') : [];
    const next = on ? [...list.filter((x) => x !== item), item] : list.filter((x) => x !== item);
    set(spec.id, next.length ? next : undefined);
  }

  function listText(v: ParamValue | undefined): string {
    return Array.isArray(v) ? v.join('\n') : typeof v === 'string' ? v : '';
  }
</script>

{#snippet control(spec: ParamSpec)}
  {@const id = `${uid}-${spec.id}`}
  {@const v = value(spec.id)}
  {#if spec.kind === 'bool' && spec.off_flags.length === 0}
    <Switch {id} aria-describedby={id + '-help'} size="sm" checked={v === true} onCheckedChange={(c) => set(spec.id, c ? true : undefined)} />
  {:else if spec.kind === 'bool'}
    <NativeSelect
      {id} aria-describedby={id + '-help'}
      value={v === true ? 'on' : v === false ? 'off' : ''}
      onchange={(e) => {
        const s = e.currentTarget.value;
        set(spec.id, s === 'on' ? true : s === 'off' ? false : undefined);
      }}
    >
      <option value="">{t('caps.option_default')}</option>
      <option value="on">{t('caps.bool_on')}</option>
      <option value="off">{t('caps.bool_off')}</option>
    </NativeSelect>
  {:else if spec.kind === 'int' || spec.kind === 'float'}
    <Input
      {id} aria-describedby={id + '-help'}
      type="number"
      step={spec.kind === 'float' ? 'any' : '1'}
      min={spec.min ?? undefined}
      max={spec.max ?? undefined}
      placeholder={spec.placeholder ?? ''}
      value={typeof v === 'number' || typeof v === 'string' ? v : ''}
      oninput={(e) => num(spec, e.currentTarget.value)}
      class="font-num"
    />
  {:else if spec.kind === 'enum'}
    <NativeSelect {id} aria-describedby={id + '-help'} value={typeof v === 'string' ? v : ''} onchange={(e) => set(spec.id, e.currentTarget.value || undefined)}>
      <option value="">{t('caps.option_default')}</option>
      {#each capsStore.allowedValues(spec) as opt (opt)}
        <option value={opt}>{opt}</option>
      {/each}
    </NativeSelect>
  {:else if spec.kind === 'multi_enum'}
    {@const selected = Array.isArray(v) ? v : typeof v === 'string' && v ? v.split(',') : []}
    <div class="flex flex-wrap gap-x-3 gap-y-1.5">
      {#each capsStore.allowedValues(spec) as opt (opt)}
        <label class="flex cursor-pointer items-center gap-1.5 text-xs">
          <Checkbox aria-describedby={id + '-help'} checked={selected.includes(opt)} onCheckedChange={(c) => toggleMulti(spec, opt, c === true)} />
          <code class="font-mono text-xs">{opt}</code>
        </label>
      {/each}
    </div>
  {:else if spec.id === 'override_kv'}
    <MetadataOverrides bind:params />
  {:else if spec.kind === 'list'}
    <Textarea
      {id} aria-describedby={id + '-help'}
      rows={2}
      placeholder={spec.placeholder ?? ''}
      value={listText(v)}
      oninput={(e) => set(spec.id, e.currentTarget.value.split('\n'))}
      class="min-h-14 font-mono text-xs"
    />
  {:else}
    <Input
      {id} aria-describedby={id + '-help'}
      type="text"
      placeholder={spec.placeholder ?? ''}
      value={typeof v === 'string' ? v : ''}
      oninput={(e) => set(spec.id, e.currentTarget.value)}
      class="font-mono"
    />
  {/if}
{/snippet}

{#if visible.length > 0 || hiddenCount > 0}
  <div class="flex flex-col gap-3">
    {#each visible as spec (spec.id)}
      <Field
        label={t(spec.label_key as TKey)}
        id="{uid}-{spec.id}"
        help={t(spec.help_key as TKey)}
        flag={flagLabel(spec)}
        hint={spec.kind === 'list' && spec.id !== 'override_kv' ? t('caps.list_hint') : undefined}
      >
        {#snippet aside()}
          <span class="flex min-w-0 items-center gap-1.5">
            <FeatureBadge feature={spec.feature} />
          </span>
        {/snippet}
        {@render control(spec)}
      </Field>
    {/each}

    <AdvancedOnly>
      {#if advancedCount > 0}
        <Button variant="link" size="xs" class="h-auto self-start px-0 text-xs" onclick={() => (showAll = !showAll)}>
          {showAll ? t('caps.hide_advanced') : t('caps.show_advanced')}
        </Button>
      {/if}
      {#if hiddenCount > 0 && capsStore.known}
        <p class="text-xs leading-snug text-muted-foreground">{t('caps.hidden_count', { count: hiddenCount })}</p>
      {/if}
    </AdvancedOnly>
  </div>
{/if}
