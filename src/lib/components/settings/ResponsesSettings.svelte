<script lang="ts">
  // Settings > Gateway > Stateful Responses: whether `/v1/responses` keeps responses for
  // `previous_response_id` (on by default; requests can opt out with `store: false`), the
  // retention limits, and clearing what is stored. Saved through the parent's
  // `set_gateway_settings` call, on top of the stored settings.
  import { Trash2 } from '@lucide/svelte';
  import { toast } from 'svelte-sonner';
  import type {
    GatewayOverview,
    GatewaySettings,
    ResponsesCompactionSettings,
    ResponsesStoreSettings,
  } from '$lib/types';
  import { invoke } from '$lib/ipc';
  import { t } from '$lib/i18n.svelte';
  import { localizeError } from '$lib/network';
  import { formatBytes } from '$lib/format';
  import { Button } from '$lib/components/ui/button';
  import * as Dialog from '$lib/components/ui/dialog';
  import { Switch } from '$lib/components/ui/switch';
  import SettingGroup from './SettingGroup.svelte';
  import SettingRow from './SettingRow.svelte';
  import NumberField from './NumberField.svelte';

  interface Props {
    overview: GatewayOverview | null;
    busy: boolean;
    onapply: (next: GatewaySettings) => void;
    onadopt: (o: GatewayOverview) => void;
  }

  let { overview, busy, onapply, onadopt }: Props = $props();

  let saved = $derived(overview?.settings.responses ?? null);
  let draft = $state<Partial<Omit<ResponsesStoreSettings, 'compaction'>>>({});
  let cdraft = $state<Partial<ResponsesCompactionSettings>>({});
  let cdirty = $derived(
    saved !== null &&
      Object.entries(cdraft).some(
        ([k, v]) => v !== undefined && v !== saved?.compaction?.[k as keyof ResponsesCompactionSettings],
      ),
  );
  let clearing = $state(false);
  let confirmClear = $state(false);
  let stats = $derived(overview?.responses_stats ?? null);
  let dirty = $derived(
    saved !== null &&
      Object.entries(draft).some(([k, v]) => v !== undefined && v !== saved?.[k as keyof ResponsesStoreSettings]),
  );

  function set(patch: Partial<ResponsesStoreSettings>) {
    if (!overview || !saved) return;
    const all = structuredClone($state.snapshot(overview.settings) as GatewaySettings);
    onapply({ ...all, responses: { ...all.responses, ...patch } });
  }

  function setCompaction(patch: Partial<ResponsesCompactionSettings>) {
    if (!overview || !saved) return;
    const all = structuredClone($state.snapshot(overview.settings) as GatewaySettings);
    onapply({ ...all, responses: { ...all.responses, compaction: { ...all.responses.compaction, ...patch } } });
  }

  function applyCompaction() {
    setCompaction($state.snapshot(cdraft) as Partial<ResponsesCompactionSettings>);
    cdraft = {};
  }

  function applyLimits() {
    set($state.snapshot(draft) as Partial<ResponsesStoreSettings>);
    draft = {};
  }

  async function clearAll() {
    confirmClear = false;
    clearing = true;
    try {
      onadopt(await invoke<GatewayOverview>('clear_gateway_responses'));
      toast.success(t('responses.cleared'));
    } catch (e) {
      toast.error(localizeError(e));
    } finally {
      clearing = false;
    }
  }
</script>

<SettingGroup title={t('responses.title')} note={t('responses.desc')}>
  {#if saved && overview}
    <SettingRow label={t('responses.store')} hint={t('responses.store_hint')} for="resp-store">
      <Switch
        id="resp-store"
        disabled={busy || !overview.owner}
        bind:checked={() => saved?.store ?? true, (v) => set({ store: v })}
      />
    </SettingRow>
    <SettingRow label={t('responses.retention_days')} hint={t('responses.retention_days_hint')} for="resp-days">
      <NumberField
        id="resp-days"
        value={draft.retention_days ?? saved.retention_days}
        min={1}
        onchange={(v) => (draft.retention_days = v)}
      />
    </SettingRow>
    <SettingRow label={t('responses.max_stored')} hint={t('responses.max_stored_hint')} for="resp-max">
      <NumberField id="resp-max" value={draft.max_stored ?? saved.max_stored} min={1} onchange={(v) => (draft.max_stored = v)} />
    </SettingRow>
    <SettingRow label={t('responses.max_mib')} hint={t('responses.max_mib_hint')} for="resp-mib">
      <NumberField id="resp-mib" value={draft.max_mib ?? saved.max_mib} min={1} onchange={(v) => (draft.max_mib = v)} />
    </SettingRow>
    {#if dirty}
      <div class="flex justify-end px-4 py-2">
        <Button size="sm" disabled={busy} onclick={applyLimits}>{t('gw.apply')}</Button>
      </div>
    {/if}
    <SettingRow label={t('responses.compact_auto')} hint={t('responses.compact_auto_hint')} for="resp-compact">
      <Switch
        id="resp-compact"
        disabled={busy || !overview.owner}
        bind:checked={() => saved?.compaction?.auto ?? false, (v) => setCompaction({ auto: v })}
      />
    </SettingRow>
    <SettingRow label={t('responses.compact_percent')} hint={t('responses.compact_percent_hint')} for="resp-cpct">
      <NumberField
        id="resp-cpct"
        value={cdraft.threshold_percent ?? saved.compaction?.threshold_percent ?? 80}
        min={10}
        onchange={(v) => (cdraft.threshold_percent = v)}
      />
    </SettingRow>
    <SettingRow label={t('responses.compact_keep')} hint={t('responses.compact_keep_hint')} for="resp-ckeep">
      <NumberField
        id="resp-ckeep"
        value={cdraft.keep_links ?? saved.compaction?.keep_links ?? 2}
        min={1}
        onchange={(v) => (cdraft.keep_links = v)}
      />
    </SettingRow>
    {#if cdirty}
      <div class="flex justify-end px-4 py-2">
        <Button size="sm" disabled={busy} onclick={applyCompaction}>{t('gw.apply')}</Button>
      </div>
    {/if}
    <SettingRow
      label={t('responses.stored')}
      note={stats ? t('responses.stored_value', { count: stats.count, size: formatBytes(stats.bytes) }) : undefined}
    >
      <Button variant="outline" size="sm" disabled={clearing || !stats?.count} onclick={() => (confirmClear = true)}>
        <Trash2 />
        {t('responses.clear')}
      </Button>
    </SettingRow>
  {/if}
</SettingGroup>

<Dialog.Root bind:open={confirmClear}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t('responses.clear_title')}</Dialog.Title>
      <Dialog.Description>
        {t('responses.clear_desc', { count: stats?.count ?? 0 })}
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer class="mt-2 gap-2">
      <Button variant="outline" onclick={() => (confirmClear = false)}>{t('responses.clear_cancel')}</Button>
      <Button variant="destructive" onclick={clearAll}>{t('responses.clear')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
