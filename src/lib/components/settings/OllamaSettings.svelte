<script lang="ts">
  // Settings > Gateway > Ollama-compatible API (P2): the address Ollama clients use, the
  // optional listener on the Ollama port (11434, off by default; a real Ollama there is never
  // replaced, only reported) and the /api/pull consent. Saved through the parent's
  // `set_gateway_settings` call, on top of the stored settings.
  import { TriangleAlert } from '@lucide/svelte';
  import type { GatewayOverview, GatewaySettings } from '#lib/types.js';
  import { t } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Switch } from '#lib/components/ui/switch/index.js';
  import CopyButton from '#lib/components/connections/CopyButton.svelte';
  import SettingGroup from './SettingGroup.svelte';
  import SettingRow from './SettingRow.svelte';
  import NumberField from './NumberField.svelte';

  interface Props {
    overview: GatewayOverview | null;
    busy: boolean;
    onapply: (next: GatewaySettings) => void;
  }

  let { overview, busy, onapply }: Props = $props();

  let portDraft = $state<number | null>(null);
  let saved = $derived(overview?.settings ?? null);
  let status = $derived(overview?.status ?? null);
  let address = $derived(status?.running ? (status.endpoint?.host_url ?? '') : '');
  // The switch holds what the user chose; the port itself is refused while a real Ollama is
  // installed or running there. Then it is on but not active: shown as pending, with the warning.
  let portPending = $derived(!!saved?.ollama_port_enabled && !!status?.running && !!status.ollama_warning);
  let portDirty = $derived(portDraft !== null && saved !== null && portDraft !== saved.ollama_port);

  function set(patch: Partial<GatewaySettings>) {
    if (!saved) return;
    onapply({ ...structuredClone($state.snapshot(saved) as GatewaySettings), ...patch });
  }

  function applyPort() {
    if (portDraft === null) return;
    set({ ollama_port: portDraft });
    portDraft = null;
  }
</script>

<SettingGroup title={t('ollama.title')} hint={t('ollama.desc')}>
  {#if saved && overview}
    <SettingRow label={t('ollama.address')} hint={t('ollama.address_hint')} note={!address ? t('ollama.gateway_off') : undefined}>
      {#if address}
        <code class="truncate font-mono text-xs">{address}</code>
        <CopyButton text={address} ariaLabel={t('conn.copy_url_aria', { name: t('ollama.title') })} />
      {/if}
    </SettingRow>
    {#if status?.running && status.ollama_port}
      <SettingRow label={t('ollama.card_port_badge', { port: status.ollama_port })}>
        <code class="truncate font-mono text-xs">http://{status.host}:{status.ollama_port}</code>
      </SettingRow>
    {/if}
    <SettingRow label={t('ollama.port_enable')} hint={t('ollama.port_enable_hint')} note={portPending ? t('ollama.port_pending_hint') : undefined} for="ollama-port-on">
      {#if portPending}
        <Badge variant="outline" class="h-5 border-warning/40 bg-warning/10 px-1.5 text-xs text-warning">{t('ollama.port_pending')}</Badge>
      {/if}
      <Switch
        id="ollama-port-on"
        disabled={busy || !overview.owner}
        bind:checked={() => saved?.ollama_port_enabled ?? false, (v) => set({ ollama_port_enabled: v })}
      />
    </SettingRow>
    <SettingRow label={t('ollama.port')} hint={t('ollama.port_hint')} for="ollama-port" dirty={portDirty}>
      <NumberField id="ollama-port" value={portDraft ?? saved.ollama_port} min={1024} max={65535} onchange={(v) => (portDraft = v)} />
      {#if portDirty}
        <Button size="sm" disabled={busy} onclick={applyPort}>{t('gw.apply')}</Button>
      {/if}
    </SettingRow>
    {#if status?.ollama_warning}
      <p class="flex items-start gap-1.5 px-4 py-2 text-xs text-warning" role="alert">
        <TriangleAlert class="mt-0.5 size-3.5 shrink-0" />
        <span>{localizeError(status.ollama_warning)}</span>
      </p>
    {/if}
    <SettingRow label={t('ollama.allow_pull')} note={t('ollama.allow_pull_hint')} help={t('ollama.allow_pull_help')} for="ollama-pull">
      <Switch
        id="ollama-pull"
        disabled={busy || !overview.owner}
        bind:checked={() => saved?.ollama_allow_pull ?? false, (v) => set({ ollama_allow_pull: v })}
      />
    </SettingRow>
    <SettingRow label={t('ollama.names_title')} hint={t('ollama.names_desc')} note={t('ollama.keep_alive_note')} />
  {/if}
</SettingGroup>
