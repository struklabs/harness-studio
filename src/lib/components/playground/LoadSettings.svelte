<script lang="ts">
  // "Load Settings" of the Playground model picker: edit the context, GPU layers, KV cache
  // type, flash attention and parallel slots of one definition for this load (or save them to
  // the definition), with the memory estimate of the launch the load would start. A model
  // another engine runs (ONNX, whisper.cpp, sd.cpp, python-hf...) shows that engine's settings.
  import { invoke, invokeShared } from '#lib/ipc.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { app, DEFAULT_SERVER_ID } from '#lib/stores/app.svelte.js';
  import { capsStore } from '#lib/stores/caps.svelte.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import type { HardwareFitResult, LaunchConfig, LoadTarget, ModelDefinition, FitResult, ParamCatalog, ParamSpec, ParamValue, ParamValues } from '#lib/types.js';
  import { toHardwareFit } from '#lib/fit.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import Field from '../dashboard/parts/Field.svelte';
  import NativeSelect from '../dashboard/parts/NativeSelect.svelte';
  import SwitchRow from '../dashboard/advanced/SwitchRow.svelte';
  import { Switch } from '#lib/components/ui/switch/index.js';
  import FitIndicator from '../dashboard/model/FitIndicator.svelte';
  import {
    differsFromRunning, formFromDefinition, formValid, kvOptions, overridesFromForm, parseCount, sameForm, visibleFields,
    type KvChoice, type LoadForm, type RunningLoad,
  } from './loadSettings';

  interface Props {
    def: ModelDefinition;
    /** The definition runs on this server (a `Running` one), if any: "Reload" targets it. */
    runningServerId: string | null;
    busy: boolean;
    /** Load once with `overrides` (the definition stays unchanged). */
    onLoad: (target: LoadTarget, overrides: ParamValues) => void;
    /** Save `overrides` to the definition, then load it. */
    onLoadAndSave: (target: LoadTarget, overrides: ParamValues) => void;
  }

  let { def, runningServerId, busy, onLoad, onLoadAndSave }: Props = $props();

  const uid = $props.id();
  const KV_LABEL = { f16: 'dash.adv_cache_f16', q8_0: 'dash.adv_cache_q8', q4_0: 'dash.adv_cache_q4' } as const;

  // The engine that runs the definition's model and its catalog (llama.cpp: the fields below).
  let engine = $state('llama.cpp');
  let engineCatalog = $state<ParamCatalog | null>(null);
  $effect(() => {
    const id = def.id;
    let live = true;
    engine = 'llama.cpp';
    engineCatalog = null;
    (async () => {
      const config = await invokeShared<LaunchConfig>('get_definition_form', { definitionId: id });
      const modelPath = config?.model_path;
      if (!modelPath) return;
      const family = (await invoke<string | null>('get_model_engine', { modelPath })) ?? 'llama.cpp';
      const catalog = family === 'llama.cpp' ? null : await invoke<ParamCatalog>('get_param_catalog', { modelPath });
      if (!live) return;
      engine = family;
      engineCatalog = catalog;
    })().catch(() => {});
    return () => {
      live = false;
    };
  });
  let otherEngine = $derived(engine !== 'llama.cpp');
  /** The engine's parameters this level shows (typed launch fields are not catalog values). */
  let engineSpecs = $derived(
    (engineCatalog?.param ?? []).filter((p) => !p.legacy_field && (uiLevel.level === 'advanced' || p.level === 'basic')),
  );
  /** Edited engine values ('' = back to automatic). */
  let engineEdits = $state<ParamValues>({});
  $effect(() => {
    void def.id;
    engineEdits = {};
  });
  function engineValue(id: string): ParamValue | undefined {
    return id in engineEdits ? engineEdits[id] : def.load[id];
  }
  let engineOverrides = $derived.by(() => {
    const out: ParamValues = {};
    for (const spec of engineSpecs) {
      if (!(spec.id in engineEdits)) continue;
      const next = engineEdits[spec.id];
      const was = def.load[spec.id];
      if (JSON.stringify(next ?? '') !== JSON.stringify(was ?? '')) out[spec.id] = next ?? '';
    }
    return out;
  });
  function setEngine(spec: ParamSpec, raw: ParamValue | undefined) {
    engineEdits = { ...engineEdits, [spec.id]: raw === undefined ? '' : raw };
  }
  function setEngineNumber(spec: ParamSpec, raw: string) {
    const n = Number(raw);
    setEngine(spec, raw.trim() === '' ? '' : Number.isFinite(n) ? n : raw);
  }

  let base = $derived(formFromDefinition(def.load));
  // svelte-ignore state_referenced_locally
  let form = $state<LoadForm>(formFromDefinition(def.load));
  let level = $derived(uiLevel.level);
  let shown = $derived(visibleFields(level));
  let overrides = $derived(otherEngine ? engineOverrides : overridesFromForm(form, base, level));
  let valid = $derived(otherEngine || formValid(form, level));
  let unchanged = $derived(Object.keys(overrides).length === 0 || (!otherEngine && sameForm(form, base)));

  // The definition changed under us (saved from here or elsewhere): start from it again.
  let baseKey = $derived(JSON.stringify(def.load));
  let seenKey = $state<string | null>(null);
  $effect(() => {
    if (seenKey !== null && seenKey !== baseKey) form = { ...base };
    seenKey = baseKey;
  });

  // Capabilities of the runtime the load uses (cached per server): which KV types it lists.
  $effect(() => {
    void capsStore.load(app.selectedServerId);
  });
  let kvTypes = $derived.by(() => {
    const spec = capsStore.spec('cache_type_k');
    return kvOptions(spec ? capsStore.allowedValues(spec) : []);
  });
  let kvSupported = $derived(capsStore.supports('cache_type_k'));
  let flashSupported = $derived(capsStore.supports('flash_attn'));
  let slotsSupported = $derived(capsStore.supports('parallel_slots'));

  /** The selected server can take the load (a profile's server runs that profile's model only). */
  let selectedIsProfile = $derived(
    app.selectedServerId !== DEFAULT_SERVER_ID && app.profiles.some((p) => p.id === app.selectedServerId)
  );
  let targetKind = $state<'server' | 'new_server'>('new_server');
  $effect(() => {
    if (selectedIsProfile && targetKind === 'server') targetKind = 'new_server';
  });
  let target = $derived<LoadTarget>(
    targetKind === 'server' && !selectedIsProfile
      ? { kind: 'server', server_id: app.selectedServerId }
      : { kind: 'new_server' }
  );

  // What the running server was started with: "Reload" is offered when it differs.
  let running = $state<RunningLoad | null>(null);
  $effect(() => {
    const id = runningServerId;
    running = null;
    if (!id) return;
    let live = true;
    invoke<LaunchConfig | null>('get_server_launch_config', { serverId: id })
      .then((c) => {
        if (live) running = c;
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  });
  let reloadId = $derived(
    runningServerId && !app.profiles.some((p) => p.id === runningServerId && p.id !== DEFAULT_SERVER_ID)
      ? runningServerId
      : null
  );
  let differs = $derived(otherEngine ? !!running && !unchanged : differsFromRunning(form, running, level));

  // Live memory estimate of the launch these settings would start (debounced).
  let fit = $state<HardwareFitResult | null>(null);
  let fitToken = 0;
  $effect(() => {
    const id = def.id;
    const ov = overrides;
    const ok = valid;
    if (!ok) {
      fit = null;
      return;
    }
    const mine = ++fitToken;
    const timer = setTimeout(() => {
      invoke<FitResult>('estimate_definition_load_fit', { definitionId: id, overrides: ov })
        .then((r) => {
          if (mine === fitToken) fit = toHardwareFit(r);
        })
        .catch(() => {
          if (mine === fitToken) fit = null;
        });
    }, 250);
    return () => clearTimeout(timer);
  });

  /** `aria-invalid` of a number field. */
  const parseInvalid = (text: string) => parseCount(text) === undefined;

  function setKv(e: Event) {
    form.kv = (e.currentTarget as HTMLSelectElement).value as KvChoice;
  }
</script>

<div class="flex flex-col gap-3 border-t border-border/60 bg-muted/30 px-3 py-3" role="group" aria-label={t('pgm.settings_for', { alias: def.alias })}>
  {#if otherEngine}
    <div class="flex flex-col gap-3">
      {#each engineSpecs as spec (spec.id)}
        {@const v = engineValue(spec.id)}
        {#if spec.kind === 'bool'}
          <Field label={t(spec.label_key as TKey)} id="{uid}-e-{spec.id}" help={t(spec.help_key as TKey)} flag={spec.flags[0]}>
            <Switch id="{uid}-e-{spec.id}" size="sm" checked={v === true} onCheckedChange={(c) => setEngine(spec, c ? true : '')} />
          </Field>
        {:else if spec.kind === 'enum'}
          <Field label={t(spec.label_key as TKey)} id="{uid}-e-{spec.id}" help={t(spec.help_key as TKey)} flag={spec.flags[0]}>
            <NativeSelect id="{uid}-e-{spec.id}" value={typeof v === 'string' ? v : ''} onchange={(e) => setEngine(spec, e.currentTarget.value)}>
              <option value="">{t('caps.option_default')}</option>
              {#each spec.values as opt (opt)}<option value={opt}>{opt}</option>{/each}
            </NativeSelect>
          </Field>
        {:else if spec.kind === 'int' || spec.kind === 'float'}
          <Field label={t(spec.label_key as TKey)} id="{uid}-e-{spec.id}" help={t(spec.help_key as TKey)} flag={spec.flags[0]}>
            <Input id="{uid}-e-{spec.id}" type="number" step={spec.kind === 'float' ? 'any' : '1'} min={spec.min ?? undefined} max={spec.max ?? undefined} placeholder={t('pgm.set_auto')} value={typeof v === 'number' || typeof v === 'string' ? v : ''} oninput={(e) => setEngineNumber(spec, e.currentTarget.value)} class="font-num" />
          </Field>
        {:else if spec.kind === 'string' || spec.kind === 'path'}
          <Field label={t(spec.label_key as TKey)} id="{uid}-e-{spec.id}" help={t(spec.help_key as TKey)} flag={spec.flags[0]}>
            <Input id="{uid}-e-{spec.id}" type="text" placeholder={spec.placeholder ?? ''} value={typeof v === 'string' ? v : ''} oninput={(e) => setEngine(spec, e.currentTarget.value)} class="font-mono" />
          </Field>
        {/if}
      {:else}
        <p class="text-xs text-muted-foreground">{t('dash.engine_none', { engine })}</p>
      {/each}
    </div>
  {:else}
  <div class="grid grid-cols-2 gap-3">
    {#if shown.includes('context')}
      <Field label={t('dash.lp_ctx_label_short')} id="{uid}-ctx" hint={t('pgm.set_context_hint')} flag="-c">
        <Input id="{uid}-ctx" inputmode="numeric" placeholder={t('pgm.set_auto')} bind:value={form.context} class="font-num" aria-invalid={parseInvalid(form.context)} />
      </Field>
    {/if}
    {#if shown.includes('gpuLayers')}
      <Field label={t('pgm.set_gpu_layers')} id="{uid}-ngl" hint={t('pgm.set_gpu_layers_hint')} flag="-ngl">
        <Input id="{uid}-ngl" inputmode="numeric" placeholder={t('pgm.set_auto')} bind:value={form.gpuLayers} class="font-num" aria-invalid={parseInvalid(form.gpuLayers)} />
      </Field>
    {/if}
    {#if shown.includes('kv') && kvSupported}
      <Field label={t('pgm.set_kv')} id="{uid}-kv" hint={t('pgm.set_kv_hint')} flag="-ctk / -ctv">
        <NativeSelect id="{uid}-kv" value={form.kv} onchange={setKv}>
          <option value="auto">{t('kv.auto')}</option>
          {#each kvTypes as k (k)}<option value={k}>{t(KV_LABEL[k])}</option>{/each}
          {#if base.kv === 'definition'}<option value="definition">{t('pgm.set_kv_definition')}</option>{/if}
        </NativeSelect>
      </Field>
    {/if}
    {#if shown.includes('slots') && slotsSupported}
      <Field label={t('pgm.set_slots')} id="{uid}-np" flag="-np">
        <Input id="{uid}-np" inputmode="numeric" placeholder={t('pgm.set_auto')} bind:value={form.slots} class="font-num" aria-invalid={parseInvalid(form.slots)} />
      </Field>
    {/if}
  </div>
  {#if shown.includes('flashAttn') && flashSupported}
    <SwitchRow id="{uid}-fa" label={t('pgm.set_flash')} flag="-fa" bind:checked={form.flashAttn} />
  {/if}
  {/if}

  {#if !valid}
    <p class="text-xs text-destructive" role="alert">{t('pgm.set_invalid')}</p>
  {/if}

  {#if fit}
    <FitIndicator fitResult={fit} recommendedNgl={null} gpuLayers={fit.offloadedLayers} onApplyRecommendedNgl={() => {}} />
  {/if}

  <div class="flex flex-wrap items-center gap-2">
    {#if !selectedIsProfile}
      <label class="flex items-center gap-1.5 text-xs text-muted-foreground" for="{uid}-target">
        {t('pgm.set_target')}
        <NativeSelect id="{uid}-target" bind:value={targetKind} class="h-7 w-auto text-xs">
          <option value="new_server">{t('pgm.load_new')}</option>
          <option value="server">{t('pgm.load_here', { server: app.serverLabel(app.selectedServerId) })}</option>
        </NativeSelect>
      </label>
    {/if}
    <span class="ml-auto flex flex-wrap items-center gap-2">
      {#if !unchanged}
        <Button variant="ghost" size="xs" onclick={() => { form = { ...base }; engineEdits = {}; }}>{t('pgm.set_reset')}</Button>
      {/if}
      {#if reloadId && differs}
        <Button size="xs" variant="outline" disabled={busy || !valid} onclick={() => onLoad({ kind: 'server', server_id: reloadId }, overrides)} title={t('pgm.set_loaded_differs')}>
          {t('pgm.reload')}
        </Button>
      {:else if !runningServerId || !reloadId}
        <Button size="xs" variant="outline" disabled={busy || !valid} onclick={() => onLoad(target, overrides)} title={t('pgm.set_load_once')}>
          {t('pgm.load')}
        </Button>
      {/if}
      <Button size="xs" disabled={busy || !valid || unchanged} onclick={() => onLoadAndSave(reloadId && differs ? { kind: 'server', server_id: reloadId } : target, overrides)}>
        {t('pgm.load_save')}
      </Button>
    </span>
  </div>
  {#if runningServerId && differs}
    <p class="text-xs text-warning" role="status">{t('pgm.set_loaded_differs')}</p>
  {:else if unchanged}
    <p class="text-xs text-muted-foreground">{t('pgm.set_unchanged')}</p>
  {/if}
</div>
