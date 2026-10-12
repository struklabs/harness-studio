<script lang="ts">
  import FieldHelp from '$lib/components/common/FieldHelp.svelte';
  // Settings > Developer API > Named tokens: create (value shown once), edit scopes and the
  // gateway policy (allowed models and endpoints, rate limits), revoke. Validated again in the
  // core (harness_core::control_api / token_policy).
  import { onMount } from 'svelte';
  import { invoke } from '$lib/ipc';
  import { toast } from 'svelte-sonner';
  import { Copy, KeyRound, Pencil, Plus, Trash2 } from '@lucide/svelte';
  import type { ApiTokenInfo, ApiTokenUpdate, CreatedApiToken } from '$lib/types';
  import { t, locale } from '$lib/i18n.svelte';
  import { localizeError } from '$lib/network';
  import { app } from '$lib/stores/app.svelte';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import SettingGroup from './SettingGroup.svelte';
  import TokenFields from './TokenFields.svelte';
  import {
    ENDPOINTS,
    draftOf,
    emptyDraft,
    noEndpoint,
    policyOf,
    selectedScopes,
    type TokenDraft,
  } from './tokenPolicy';

  interface Props {
    /** Called with the number of named tokens after every change. */
    oncount?: (n: number) => void;
  }

  let { oncount }: Props = $props();

  let tokens = $state<ApiTokenInfo[]>([]);
  let createDraft = $state<TokenDraft>(emptyDraft());
  let creating = $state(false);
  let created = $state<CreatedApiToken | null>(null);
  let createError = $state<string | null>(null);
  let editing = $state<string | null>(null);
  let editDraft = $state<TokenDraft>(emptyDraft());
  let editError = $state<string | null>(null);
  let confirmRevoke = $state<string | null>(null);
  let busy = $state(false);

  /** The gateway takes requests without a token: the policy binds only token holders. */
  let gatewayOpen = $derived.by(() => {
    const g = app.gateway;
    if (!g) return false;
    return !(g.status.running ? g.status.require_token : g.settings.require_token);
  });

  async function reload() {
    tokens = await invoke<ApiTokenInfo[]>('list_api_tokens');
    oncount?.(tokens.length);
  }

  onMount(() => {
    reload().catch((e) => (createError = localizeError(e)));
    app.refreshGateway(true);
  });

  function invalid(d: TokenDraft): boolean {
    return selectedScopes(d).length === 0 || policyOf(d) === null || noEndpoint(d);
  }

  async function create() {
    const policy = policyOf(createDraft);
    if (!policy) return;
    busy = true;
    createError = null;
    try {
      created = await invoke<CreatedApiToken>('create_api_token', {
        name: createDraft.name,
        scopes: selectedScopes(createDraft),
        policy,
      });
      createDraft = emptyDraft();
      creating = false;
      await reload();
    } catch (e) {
      createError = localizeError(e);
    } finally {
      busy = false;
    }
  }

  function startEdit(tok: ApiTokenInfo) {
    editing = tok.id;
    editDraft = draftOf(tok);
    editError = null;
    confirmRevoke = null;
  }

  async function saveEdit(id: string) {
    const policy = policyOf(editDraft);
    if (!policy) return;
    const change: ApiTokenUpdate = {
      name: editDraft.name,
      scopes: selectedScopes(editDraft),
      ...policy,
    };
    busy = true;
    editError = null;
    try {
      await invoke<ApiTokenInfo>('update_api_token', { id, change });
      editing = null;
      await reload();
      toast.success(t('dev.token_saved'));
    } catch (e) {
      editError = localizeError(e);
    } finally {
      busy = false;
    }
  }

  async function revoke(id: string) {
    if (confirmRevoke !== id) {
      confirmRevoke = id;
      return;
    }
    confirmRevoke = null;
    try {
      await invoke('revoke_api_token', { id });
      if (editing === id) editing = null;
      await reload();
      toast.success(t('dev.token_revoked'));
    } catch (e) {
      toast.error(localizeError(e));
    }
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

  const fmtDate = (secs: number) => new Date(secs * 1000).toLocaleString(locale());

  function limitSummary(tok: ApiTokenInfo): string {
    const l = tok.limits;
    const parts: string[] = [];
    if (l.requests_per_minute != null) parts.push(t('dev.summary_rpm', { n: l.requests_per_minute }));
    if (l.tokens_per_minute != null) parts.push(t('dev.summary_tpm', { n: l.tokens_per_minute }));
    if (l.max_concurrent != null) parts.push(t('dev.summary_conc', { n: l.max_concurrent }));
    return parts.length ? parts.join(' · ') : t('dev.summary_no_limits');
  }

  function endpointSummary(tok: ApiTokenInfo): string {
    if (tok.endpoints.length === 0) return t('dev.summary_all_endpoints');
    return ENDPOINTS.filter((e) => tok.endpoints.includes(e.id))
      .map((e) => t(e.label))
      .join(', ');
  }

  const gatewayToken = (tok: ApiTokenInfo) => tok.scopes.includes('inference') || tok.scopes.includes('admin');
</script>

<SettingGroup title={t('dev.tokens_title')} note={t('dev.tokens_desc')}>
  {#snippet actions()}
    {#if !creating}
      <Button size="sm" variant="outline" onclick={() => ((creating = true), (createError = null))}>
        <Plus />
        {t('dev.token_new')}
      </Button>
    {/if}
  {/snippet}

  {#if created}
    <div class="flex flex-col gap-2 border-l-2 border-warning bg-warning/10 px-4 py-3" role="status">
      <p class="text-xs font-medium">{t('dev.token_created')}</p>
      <div class="flex min-w-0 flex-wrap items-center gap-2">
        <code class="min-w-0 flex-1 truncate rounded bg-muted px-2 py-1 font-mono text-xs select-all">{created.token}</code>
        <Button variant="outline" size="sm" onclick={() => copy(created?.token)}><Copy /> {t('dev.copy')}</Button>
        <Button variant="ghost" size="sm" onclick={() => (created = null)}>{t('dev.token_done')}</Button>
      </div>
    </div>
  {/if}

  {#if creating}
    <div class="flex flex-col gap-3 px-4 py-3">
      <div class="flex flex-col gap-1">
        <label for="dev-token-name" class="text-xs font-medium">{t('dev.token_name')}</label>
        <FieldHelp text={t('help.token_name')}><Input
          id="dev-token-name"
          class="h-8 max-w-72 text-xs"
          placeholder={t('dev.token_name_placeholder')}
          autocomplete="off"
          bind:value={createDraft.name}
        /></FieldHelp>
      </div>
      <TokenFields bind:draft={createDraft} idPrefix="dev-token-new" {gatewayOpen} />
      {#if createError}<p class="text-xs text-destructive" role="alert">{createError}</p>{/if}
      <div class="flex flex-wrap justify-end gap-2">
        <Button variant="ghost" size="sm" onclick={() => ((creating = false), (createDraft = emptyDraft()))}>
          {t('settings.cancel')}
        </Button>
        <Button size="sm" disabled={busy || !createDraft.name.trim() || invalid(createDraft)} onclick={create}>
          <KeyRound />
          {t('dev.token_create')}
        </Button>
      </div>
    </div>
  {/if}

  {#each tokens as tok (tok.id)}
    <div class="flex flex-col gap-2 px-4 py-3">
      <div class="flex min-w-0 flex-wrap items-start justify-between gap-x-4 gap-y-2">
        <div class="flex min-w-0 flex-col gap-1">
          <p class="truncate text-sm font-medium">{tok.name}</p>
          <p class="text-xs text-muted-foreground">{t('dev.token_created_at', { date: fmtDate(tok.created_at) })}</p>
          <div class="flex flex-wrap gap-1">
            {#each tok.scopes as s (s)}<Badge variant="outline" class="font-mono">{s}</Badge>{/each}
          </div>
          {#if gatewayToken(tok)}
            <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-xs">
              <dt class="text-muted-foreground">{t('dev.policy_models')}</dt>
              <dd class="min-w-0 truncate font-mono">
                {tok.models.length ? tok.models.join(', ') : t('dev.summary_all_models')}
              </dd>
              <dt class="text-muted-foreground">{t('dev.policy_endpoints')}</dt>
              <dd class="min-w-0 truncate">{endpointSummary(tok)}</dd>
              <dt class="text-muted-foreground">{t('dev.limits')}</dt>
              <dd class="min-w-0 truncate tabular">{limitSummary(tok)}</dd>
            </dl>
          {/if}
        </div>
        <div class="flex shrink-0 flex-wrap items-center gap-2">
          {#if editing !== tok.id}
            <Button variant="outline" size="sm" onclick={() => startEdit(tok)}>
              <Pencil />
              {t('dev.token_edit')}
            </Button>
          {/if}
          <Button variant={confirmRevoke === tok.id ? 'destructive' : 'outline'} size="sm" onclick={() => revoke(tok.id)}>
            <Trash2 />
            {t('dev.token_revoke')}
          </Button>
          {#if confirmRevoke === tok.id}
            <Button variant="ghost" size="sm" onclick={() => (confirmRevoke = null)}>{t('settings.cancel')}</Button>
          {/if}
        </div>
      </div>
      {#if confirmRevoke === tok.id}
        <p class="text-xs text-warning" role="alert">{t('dev.token_revoke_confirm')}</p>
      {/if}
      {#if editing === tok.id}
        <div class="flex flex-col gap-3 rounded-md border border-border p-3">
          <div class="flex flex-col gap-1">
            <label for="dev-token-edit-{tok.id}" class="text-xs font-medium">{t('dev.token_name')}</label>
            <FieldHelp text={t('help.token_name')}><Input
              id="dev-token-edit-{tok.id}"
              class="h-8 max-w-72 text-xs"
              autocomplete="off"
              bind:value={editDraft.name}
            /></FieldHelp>
          </div>
          <TokenFields bind:draft={editDraft} idPrefix="dev-token-{tok.id}" {gatewayOpen} />
          {#if editError}<p class="text-xs text-destructive" role="alert">{editError}</p>{/if}
          <div class="flex flex-wrap justify-end gap-2">
            <Button variant="ghost" size="sm" onclick={() => (editing = null)}>{t('settings.cancel')}</Button>
            <Button size="sm" disabled={busy || !editDraft.name.trim() || invalid(editDraft)} onclick={() => saveEdit(tok.id)}>
              {t('dev.token_save')}
            </Button>
          </div>
        </div>
      {/if}
    </div>
  {:else}
    {#if !creating}
      <p class="px-4 py-3 text-xs text-muted-foreground">{t('dev.tokens_none')}</p>
    {/if}
  {/each}
</SettingGroup>
