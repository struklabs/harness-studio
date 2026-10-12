<script lang="ts">
  import { untrack } from 'svelte';
  import { t } from '$lib/i18n.svelte';
  import { Input } from '$lib/components/ui/input';
  import { Button } from '$lib/components/ui/button';
  import { capsStore } from '$lib/stores/caps.svelte';
  import type { LaunchParamsForm } from '../params';
  import { METADATA_TYPES, parseMetadataOverrides, serializeMetadataOverrides, validMetadataOverride, type MetadataOverrideRow, type MetadataType } from '../metadataOverrides';
  import NativeSelect from '../parts/NativeSelect.svelte';

  let { params = $bindable(), selectedModelId = '' }: { params: LaunchParamsForm; selectedModelId?: string } = $props();
  const uid = $props.id();
  let rows = $state<MetadataOverrideRow[]>([]);
  let lastValue = '';
  let lastModel = '';
  let supported = $derived(capsStore.supports('override_kv'));
  $effect(() => {
    const model = selectedModelId;
    const value = params.catalog_params.override_kv;
    const signature = JSON.stringify(value ?? []);
    untrack(() => {
      if (signature !== lastValue || model !== lastModel) {
        rows = parseMetadataOverrides(value);
        lastValue = signature;
        lastModel = model;
      }
    });
  });

  function save() {
    const values = serializeMetadataOverrides(rows);
    lastValue = JSON.stringify(values);
    if (values.length) params.catalog_params.override_kv = values;
    else delete params.catalog_params.override_kv;
  }
  function update(index: number, patch: Partial<MetadataOverrideRow>) {
    rows[index] = { ...rows[index], ...patch };
    save();
  }
  function remove(index: number) {
    rows = rows.filter((_, i) => i !== index);
    save();
  }
</script>

<div class="flex flex-col gap-2">
  <span id="{uid}-help" class="sr-only">{t('param.override_kv_help')}</span>
  {#each rows as row, i}
    <div class="grid grid-cols-[minmax(0,1fr)_5rem_minmax(0,0.7fr)_auto] items-end gap-2">
      <label for="{uid}-key-{i}" class="flex min-w-0 flex-col gap-1 text-xs">
        {t('param.override_kv_key')}
        <Input aria-describedby="{uid}-help" id="{uid}-key-{i}" value={row.key} disabled={!supported} oninput={(e) => update(i, { key: e.currentTarget.value })} />
      </label>
      <label for="{uid}-type-{i}" class="flex flex-col gap-1 text-xs">
        {t('param.override_kv_type')}
        <NativeSelect aria-describedby="{uid}-help" id="{uid}-type-{i}" value={row.type} disabled={!supported} onchange={(e) => update(i, { type: e.currentTarget.value as MetadataType, value: e.currentTarget.value === 'bool' ? 'true' : row.value })}>
          {#each METADATA_TYPES as type}<option value={type}>{type}</option>{/each}
        </NativeSelect>
      </label>
      <label for="{uid}-value-{i}" class="flex min-w-0 flex-col gap-1 text-xs">
        {t('param.override_kv_value')}
        {#if row.type === 'bool'}
          <NativeSelect aria-describedby="{uid}-help" id="{uid}-value-{i}" value={row.value} disabled={!supported} onchange={(e) => update(i, { value: e.currentTarget.value })}>
            <option value="true">true</option><option value="false">false</option>
          </NativeSelect>
        {:else}
          <Input aria-describedby="{uid}-help" id="{uid}-value-{i}" type={row.type === 'str' ? 'text' : 'number'} step={row.type === 'int' ? '1' : 'any'} value={row.value} disabled={!supported} oninput={(e) => update(i, { value: e.currentTarget.value })} />
        {/if}
      </label>
      <Button variant="ghost" size="sm" onclick={() => remove(i)}>{t('param.override_kv_remove')}</Button>
    </div>
  {/each}
  <Button variant="outline" size="sm" class="self-start" disabled={!supported} onclick={() => (rows = [...rows, { key: '', type: 'int', value: '' }])}>{t('param.override_kv_add')}</Button>
  {#if rows.some((row) => !validMetadataOverride(row))}
    <p class="text-xs text-warning" role="status">{t('param.override_kv_invalid')}</p>
  {/if}
  {#if !supported}
    <p class="text-xs text-muted-foreground" role="status">{t('param.override_kv_unsupported')}</p>
  {/if}
</div>
