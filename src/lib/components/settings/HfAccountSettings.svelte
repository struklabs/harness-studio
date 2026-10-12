<script lang="ts">
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import FieldHelp from '$lib/components/common/FieldHelp.svelte';
  import { onMount } from 'svelte';
  import { invoke } from '$lib/ipc';
  import { BackendError } from '$lib/errors';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { toast } from 'svelte-sonner';
  import { ChevronRight, Eye, EyeOff, ExternalLink, KeyRound, LoaderCircle, LogIn, LogOut } from '@lucide/svelte';
  import type { AppSettings, HfAuthStatus, HfUser } from '$lib/types';
  import { t, locale } from '$lib/i18n.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Badge } from '$lib/components/ui/badge';
  import PublisherAvatar from '$lib/components/hub/PublisherAvatar.svelte';
  import SettingGroup from './SettingGroup.svelte';
  import SettingRow from './SettingRow.svelte';

  interface Props {
    settings: AppSettings;
    onSettingsUpdated: (updated: AppSettings) => void;
  }

  let { settings, onSettingsUpdated }: Props = $props();

  const CONNECTED_APPS_URL = 'https://huggingface.co/settings/connected-applications';
  const TOKENS_URL = 'https://huggingface.co/settings/tokens';
  const NEW_APP_URL = 'https://huggingface.co/settings/applications/new';

  let status = $state<HfAuthStatus | null>(null);
  let statusError = $state<string | null>(null);
  /** Account of a pasted token (the OAuth profile comes with the status). */
  let tokenUser = $state<HfUser | null>(null);
  let checkingUser = $state(false);
  let userError = $state<string | null>(null);
  let signingIn = $state(false);
  let signingOut = $state(false);

  // Access token: write-only. The backend keeps it in the OS keychain and only reports `hf_token_set`.
  let hfTokenInput = $state('');
  let showToken = $state(false);
  let hfBusy = $state(false);
  let hfError = $state<string | null>(null);
  let tokenSet = $derived(settings.hf_token_set === true);

  let clientIdInput = $state('');
  let clientIdBusy = $state(false);
  let clientIdError = $state<string | null>(null);

  let user = $derived(status?.kind === 'oauth' ? status.user : tokenUser);

  function snapshot(): AppSettings {
    return structuredClone($state.snapshot(settings) as AppSettings);
  }

  function open(url: string) {
    openUrl(url).catch((e) => toast.error(t('hub.open_link_failed', { error: String(e) })));
  }

  async function refreshStatus() {
    try {
      status = await invoke<HfAuthStatus>('hf_auth_status');
      statusError = null;
    } catch (e) {
      statusError = t('hfauth.status_failed', { error: String(e) });
    }
    if (status?.kind === 'token') void checkTokenUser();
    else tokenUser = null;
  }

  async function checkTokenUser() {
    checkingUser = true;
    userError = null;
    try {
      tokenUser = await invoke<HfUser>('hf_whoami');
    } catch (e) {
      tokenUser = null;
      userError = t('hfauth.account_check_failed', { error: String(e) });
    } finally {
      checkingUser = false;
    }
  }

  onMount(() => {
    clientIdInput = settings.hf_oauth_client_id ?? '';
    void refreshStatus();
  });

  async function signIn() {
    if (signingIn) return;
    signingIn = true;
    try {
      status = await invoke<HfAuthStatus>('hf_oauth_login');
      onSettingsUpdated({ ...snapshot(), hf_token_set: true });
      tokenUser = null;
      if (status.user) toast.success(t('hfauth.signed_in_toast', { name: status.user.name }));
    } catch (e) {
      // A cancel is the user's own action: no error toast.
      if (!(e instanceof BackendError && e.code === 'hf_auth.cancelled')) {
        toast.error(t('hfauth.sign_in_failed', { error: String(e) }));
      }
    } finally {
      signingIn = false;
    }
  }

  async function cancelSignIn() {
    await invoke<boolean>('hf_oauth_cancel').catch(() => false);
  }

  async function signOut() {
    if (signingOut) return;
    signingOut = true;
    try {
      const updated = await invoke<AppSettings>('hf_logout');
      onSettingsUpdated({ ...snapshot(), hf_token_set: updated.hf_token_set });
      tokenUser = null;
      toast.success(t('hfauth.signed_out_toast'));
      await refreshStatus();
    } catch (e) {
      toast.error(t('hfauth.sign_out_failed', { error: String(e) }));
    } finally {
      signingOut = false;
    }
  }

  async function updateHfToken(command: 'set_hf_token' | 'clear_hf_token') {
    if (hfBusy) return;
    hfBusy = true;
    hfError = null;
    try {
      const updated = await invoke<AppSettings>(command, command === 'set_hf_token' ? { token: hfTokenInput.trim() } : {});
      hfTokenInput = '';
      showToken = false;
      onSettingsUpdated({ ...snapshot(), hf_token_set: updated.hf_token_set });
      toast.success(command === 'set_hf_token' ? t('settings.hf_token_saved_toast') : t('settings.hf_token_cleared_toast'));
      await refreshStatus();
    } catch (e) {
      hfError = t('settings.hf_token_save_failed', { error: String(e) });
    } finally {
      hfBusy = false;
    }
  }

  async function saveClientId() {
    if (clientIdBusy) return;
    clientIdBusy = true;
    clientIdError = null;
    try {
      const value = clientIdInput.trim();
      status = await invoke<HfAuthStatus>('set_hf_oauth_client_id', { clientId: value });
      onSettingsUpdated({ ...snapshot(), hf_oauth_client_id: value });
      toast.success(t('hfauth.client_id_saved'));
    } catch (e) {
      clientIdError = t('hfauth.client_id_save_failed', { error: String(e) });
    } finally {
      clientIdBusy = false;
    }
  }

  let expiresText = $derived(
    status?.kind === 'oauth' && status.expires_at
      ? new Date(status.expires_at * 1000).toLocaleString(locale(), { dateStyle: 'medium', timeStyle: 'short' })
      : null
  );
  let sourceText = $derived.by(() => {
    switch (status?.client_id_source) {
      case 'settings':
        return t('hfauth.client_id_source_settings');
      case 'env':
        return t('hfauth.client_id_source_env');
      case 'build':
        return t('hfauth.client_id_source_build');
      default:
        return t('hfauth.client_id_source_none');
    }
  });
</script>

<SettingGroup title={t('settings.hf_integration_title')} hint={t('hfauth.group_desc')}>
  <SettingRow label={t('hfauth.account_label')}>
    {#snippet extra()}
      <div class="flex flex-wrap items-center justify-between gap-3" aria-live="polite">
        <div class="flex min-w-0 items-center gap-2.5">
          {#if user}
            <PublisherAvatar name={user.name} class="size-9" />
            <div class="flex min-w-0 flex-col">
              <span class="truncate text-sm font-medium">{user.fullname ?? user.name}</span>
              <span class="truncate text-xs text-muted-foreground">
                @{user.name} · {status?.kind === 'oauth' ? t('hfauth.signed_in_oauth') : t('hfauth.signed_in_token')}
              </span>
            </div>
          {:else if checkingUser}
            <LoaderCircle class="size-4 animate-spin text-muted-foreground" />
            <span class="text-xs text-muted-foreground">{t('hfauth.checking_account')}</span>
          {:else if status?.kind === 'token'}
            <Badge variant="outline" class="text-success">{t('hfauth.signed_in_token')}</Badge>
          {:else}
            <span class="text-sm text-muted-foreground">{t('hfauth.signed_out')}</span>
          {/if}
        </div>

        <div class="flex shrink-0 flex-wrap items-center gap-2">
          {#if signingIn}
            <span class="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
              <LoaderCircle class="size-3.5 animate-spin" />{t('hfauth.waiting_browser')}
            </span>
            <Button variant="ghost" size="sm" onclick={cancelSignIn}>{t('hfauth.cancel_sign_in')}</Button>
          {:else}
            {#if status?.kind !== 'oauth'}
              <Button size="sm" disabled={!status?.oauth_available} onclick={signIn}>
                <LogIn />{t('hfauth.sign_in')}
              </Button>
            {/if}
            {#if status && status.kind !== 'none'}
              <Button variant="outline" size="sm" disabled={signingOut} onclick={signOut}>
                <LogOut />{t('hfauth.sign_out')}
              </Button>
            {/if}
          {/if}
        </div>
      </div>

      {#if status && !status.oauth_available && status.kind !== 'oauth'}
        <p class="text-xs text-muted-foreground">{t('hfauth.needs_client_id')}</p>
      {/if}
      {#if expiresText}
        <AdvancedOnly>
          <p class="text-xs text-muted-foreground">{t('hfauth.expires_at', { time: expiresText })}</p>
        </AdvancedOnly>
      {/if}
      {#if status?.kind === 'oauth'}
        <p class="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
          <span>{t('hfauth.revoke_hint')}</span>
          <button type="button" class="inline-flex items-center gap-1 text-primary hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" onclick={() => open(CONNECTED_APPS_URL)}>
            {t('hfauth.open_connected_apps')}<ExternalLink class="size-3" />
          </button>
        </p>
      {/if}
      {#if userError}<p class="text-xs break-words text-destructive" role="alert">{userError}</p>{/if}
      {#if statusError}<p class="text-xs break-words text-destructive" role="alert">{statusError}</p>{/if}
    {/snippet}
  </SettingRow>

  <SettingRow label={t('settings.hf_token_label')} for="hf-token-input" hint={t('hfauth.token_hint')}>
    {#snippet extra()}
      <form
        class="flex min-w-0 items-center gap-2"
        onsubmit={(e) => {
          e.preventDefault();
          if (hfTokenInput.trim()) updateHfToken('set_hf_token');
        }}
      >
        <div class="relative min-w-0 flex-1">
          <KeyRound class="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            id="hf-token-input"
            type={showToken ? 'text' : 'password'}
            autocomplete="off"
            spellcheck={false}
            class="h-8 pl-8 font-mono text-xs"
            placeholder={tokenSet ? t('settings.hf_token_placeholder_set') : 'hf_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'}
            bind:value={hfTokenInput}
          />
        </div>
        <Button variant="ghost" size="icon-sm" aria-label={t('settings.show_hide')} aria-pressed={showToken} onclick={() => (showToken = !showToken)}>
          {#if showToken}<EyeOff />{:else}<Eye />{/if}
        </Button>
        <Button type="submit" variant="secondary" size="sm" disabled={!hfTokenInput.trim() || hfBusy}>
          {t('settings.hf_token_save')}
        </Button>
        {#if tokenSet && status?.kind !== 'oauth'}
          <Button variant="ghost" size="sm" disabled={hfBusy} onclick={() => updateHfToken('clear_hf_token')}>
            {t('settings.hf_token_clear')}
          </Button>
        {/if}
      </form>
      <div class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        <Badge variant="outline" class={tokenSet ? 'text-success' : ''}>
          {tokenSet ? t('settings.hf_token_status_set') : t('settings.hf_token_status_unset')}
        </Badge>
        <span class="min-w-0 truncate">{tokenSet ? t('settings.hf_token_saved') : t('settings.hf_token_not_set')}</span>
        <button type="button" class="ml-auto inline-flex items-center gap-1 text-primary hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" onclick={() => open(TOKENS_URL)}>
          {t('hfauth.open_tokens_page')}<ExternalLink class="size-3" />
        </button>
      </div>
      {#if hfError}<p class="text-xs break-words text-destructive" role="alert">{hfError}</p>{/if}
    {/snippet}
  </SettingRow>

  <AdvancedOnly>
    <details class="group px-4 py-3">
      <summary class="flex w-fit cursor-pointer items-center gap-1 rounded-sm text-sm font-medium outline-none select-none focus-visible:ring-2 focus-visible:ring-ring/50">
        <ChevronRight class="size-3.5 transition-transform group-open:rotate-90" />{t('hfauth.advanced_title')}
      </summary>
      <div class="mt-3 flex flex-col gap-2">
        <p class="text-xs text-muted-foreground">
          {t('hfauth.register_steps', {
            redirect: status?.registered_redirect_uri ?? 'http://127.0.0.1/callback',
            scopes: (status?.scopes ?? 'openid profile read-repos gated-repos').split(' ').join(', '),
          })}
        </p>
        <button type="button" class="inline-flex w-fit items-center gap-1 text-xs text-primary hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" onclick={() => open(NEW_APP_URL)}>
          {t('hfauth.open_register_page')}<ExternalLink class="size-3" />
        </button>
        <label for="hf-client-id" class="mt-1 text-xs font-medium">{t('hfauth.client_id_label')}</label>
        <form
          class="flex min-w-0 items-center gap-2"
          onsubmit={(e) => {
            e.preventDefault();
            saveClientId();
          }}
        >
          <FieldHelp text={t('help.oauth_client')}><Input
            id="hf-client-id"
            autocomplete="off"
            spellcheck={false}
            class="h-8 min-w-0 flex-1 font-mono text-xs"
            placeholder={t('hfauth.client_id_placeholder')}
            bind:value={clientIdInput}
          /></FieldHelp>
          <Button type="submit" variant="secondary" size="sm" disabled={clientIdBusy || clientIdInput.trim() === (settings.hf_oauth_client_id ?? '')}>
            {t('hfauth.client_id_save')}
          </Button>
        </form>
        <p class="text-xs text-muted-foreground">
          {sourceText}{#if status?.client_id && status.client_id_source !== 'settings'}: <span class="font-mono">{status.client_id}</span>{/if}
        </p>
        {#if clientIdError}<p class="text-xs break-words text-destructive" role="alert">{clientIdError}</p>{/if}
      </div>
    </details>
  </AdvancedOnly>
</SettingGroup>
