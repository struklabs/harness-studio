<script lang="ts">
  let { page = 'developer-api' }: { page?: string } = $props();
  // Settings > Developer > Control API: the Harness Control API (off by default), its session token and
  // named tokens. Saved through its own commands (validated in the core), not the settings form.
  import { onMount } from 'svelte';
  import { invoke } from '$lib/ipc';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { toast } from 'svelte-sonner';
  import { Copy, Eye, EyeOff, ShieldAlert, ExternalLink } from '@lucide/svelte';
  import type { ControlApiOverview, ControlApiSettings } from '$lib/types';
  import { t } from '$lib/i18n.svelte';
  import { localizeError } from '$lib/network';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Switch } from '$lib/components/ui/switch';
  import { Textarea } from '$lib/components/ui/textarea';
  import SettingGroup from './SettingGroup.svelte';
  import SettingRow from './SettingRow.svelte';
  import NumberField from './NumberField.svelte';
  import CliSettings from './CliSettings.svelte';
  import ApiTokenSettings from './ApiTokenSettings.svelte';

  let overview = $state<ControlApiOverview | null>(null);
  let draft = $state<ControlApiSettings | null>(null);
  let corsText = $state('');
  let busy = $state(false);
  let applyError = $state<string | null>(null);

  let sessionToken = $state<string | null>(null);
  let revealed = $state(false);

  function adopt(o: ControlApiOverview) {
    overview = o;
    draft = { ...o.settings, cors_origins: [...o.settings.cors_origins] };
    corsText = o.settings.cors_origins.join('\n');
  }

  async function refresh() {
    try {
      adopt(await invoke<ControlApiOverview>('get_control_api_status'));
      sessionToken = await invoke<string | null>('get_control_api_token');
    } catch (e) {
      applyError = localizeError(e);
    }
  }

  onMount(refresh);

  let dirty = $derived(
    !!overview &&
      !!draft &&
      (draft.host !== overview.settings.host ||
        draft.port !== overview.settings.port ||
        draft.allow_network !== overview.settings.allow_network ||
        corsLines().join('\n') !== overview.settings.cors_origins.join('\n')),
  );

  function corsLines(): string[] {
    return corsText
      .split('\n')
      .map((s) => s.trim())
      .filter(Boolean);
  }

  async function apply(next: ControlApiSettings) {
    busy = true;
    applyError = null;
    try {
      adopt(await invoke<ControlApiOverview>('set_control_api_settings', { settings: next }));
      sessionToken = await invoke<string | null>('get_control_api_token');
      revealed = false;
      toast.success(t('dev.applied'));
    } catch (e) {
      applyError = localizeError(e);
    } finally {
      busy = false;
    }
  }

  function toggleEnabled(on: boolean) {
    if (!draft) return;
    apply({ ...draft, enabled: on, cors_origins: corsLines() });
  }

  function applyDraft() {
    if (!draft) return;
    apply({ ...draft, cors_origins: corsLines() });
  }

  async function copy(text: string | null | undefined) {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      toast.success(t('dev.copied'));
    } catch (e) {
      toast.error(String(e));
    }
  }

  const masked = (tok: string) => tok.slice(0, 4) + '•'.repeat(24);

</script>

<div class="flex flex-col gap-6">
  {#if overview && !overview.owner}
    <div class="flex items-start gap-2 rounded-md border border-warning/40 bg-warning/5 p-3 text-xs" role="status">
      <ShieldAlert class="mt-0.5 size-3.5 shrink-0 text-warning" />
      <div class="flex flex-col gap-0.5">
        <p class="font-medium">{t('dev.not_owner_title')}</p>
        <p class="text-muted-foreground">
          {t('dev.not_owner_body', { pid: overview.other_owner?.pid ?? '?' })}
        </p>
      </div>
    </div>
  {/if}

<div hidden={page !== 'developer-api'} class="flex flex-col gap-3">
  <SettingGroup title={t('dev.api_title')} showTitle={false} hint={t('dev.api_desc')}>
    {#snippet actions()}
      {#if overview?.status.running}
        <Badge variant="outline" class="text-success">{t('dev.status_running')}</Badge>
      {:else}
        <Badge variant="outline">{t('dev.status_stopped')}</Badge>
      {/if}
    {/snippet}
    {#if draft && overview}
      <SettingRow label={t('dev.enable')} hint={t('dev.enable_hint')} for="dev-api-enabled">
        <Switch
          id="dev-api-enabled"
          disabled={busy || !overview.owner}
          bind:checked={() => overview?.settings.enabled ?? false, (v) => toggleEnabled(v)}
        />
      </SettingRow>
      <SettingRow help={t('help.api_port')} label={t('dev.port')} for="dev-api-port">
        <NumberField id="dev-api-port" value={draft.port} min={1024} max={65535} onchange={(v) => draft && (draft.port = v)} />
      </SettingRow>
      {#if overview.status.running && overview.status.port_fallback}
        {@const fb = overview.status.port_fallback}
        {@const params = { configured: fb.configured, program: fb.holder?.name ?? '', port: overview.status.port ?? '' }}
        <p class="px-4 py-2 text-xs text-warning" role="status">
          {fb.holder?.name ? t('dev.port_fallback', params) : t('dev.port_fallback_unknown', params)}
        </p>
      {/if}
      <SettingRow label={t('dev.host')} hint={t('dev.host_hint')} for="dev-api-host">
        <Input
          id="dev-api-host"
          class="h-8 w-40 font-num text-xs"
          spellcheck={false}
          autocomplete="off"
          bind:value={draft.host}
        />
      </SettingRow>
      <SettingRow label={t('dev.allow_network')} note={t('dev.allow_network_hint')} help={t('help.bind_host')} for="dev-api-lan">
        <Switch id="dev-api-lan" bind:checked={draft.allow_network} />
      </SettingRow>
      <SettingRow label={t('dev.cors')} hint={t('dev.cors_hint')} help={t('help.cors')} for="dev-api-cors">
        <Textarea
          id="dev-api-cors"
          rows={2}
          class="font-mono text-xs"
          spellcheck={false}
          placeholder="http://localhost:5173"
          bind:value={corsText}
        />
      </SettingRow>
      {#if overview.status.running && overview.status.base_url}
        <SettingRow label={t('dev.base_url')}>
          <code class="truncate font-mono text-xs">{overview.status.base_url}</code>
          <Button variant="ghost" size="icon-sm" aria-label={t('dev.copy')} title={t('dev.copy')} onclick={() => copy(overview?.status.base_url)}>
            <Copy />
          </Button>
        </SettingRow>
        <SettingRow label={t('dev.openapi')}>
          <code class="max-w-72 truncate font-mono text-xs">{overview.openapi_url}</code>
          <Button variant="ghost" size="icon-sm" aria-label={t('dev.copy')} title={t('dev.copy')} onclick={() => copy(overview?.openapi_url)}>
            <Copy />
          </Button>
          <Button variant="ghost" size="sm" onclick={() => overview?.openapi_url && openUrl(overview.openapi_url)}>
            <ExternalLink />
            {t('dev.open')}
          </Button>
        </SettingRow>
      {/if}
      {#if applyError || overview.status.error}
        <p class="px-4 py-2 text-xs text-destructive" role="alert">
          {applyError ?? t('dev.status_error', { error: localizeError(overview.status.error) })}
        </p>
      {/if}
      {#if dirty}
        <div class="flex justify-end px-4 py-2">
          <Button size="sm" disabled={busy} onclick={applyDraft}>{t('dev.apply')}</Button>
        </div>
      {/if}
    {/if}
  </SettingGroup>

  <SettingGroup title={t('dev.session_title')} hint={t('dev.session_desc')}>
    {#if sessionToken}
      <SettingRow label={t('dev.session_title')} note={overview?.status.token_path ? t('dev.token_file', { path: overview.status.token_path }) : undefined}>
        <code class="max-w-72 truncate font-mono text-xs">{revealed ? sessionToken : masked(sessionToken)}</code>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={revealed ? t('dev.hide') : t('dev.reveal')}
          title={revealed ? t('dev.hide') : t('dev.reveal')}
          onclick={() => (revealed = !revealed)}
        >
          {#if revealed}<EyeOff />{:else}<Eye />{/if}
        </Button>
        <Button variant="ghost" size="icon-sm" aria-label={t('dev.copy')} title={t('dev.copy')} onclick={() => copy(sessionToken)}>
          <Copy />
        </Button>
      </SettingRow>
    {:else}
      <p class="px-4 py-3 text-xs text-muted-foreground">{t('dev.session_none')}</p>
    {/if}
  </SettingGroup>

</div>
<div hidden={page !== 'developer-cli'} class="flex flex-col gap-3">
  <CliSettings />

</div>
<div hidden={page !== 'developer-api'} class="flex flex-col gap-3">
  <ApiTokenSettings oncount={(n) => overview && (overview.named_tokens = n)} />
</div>
</div>
