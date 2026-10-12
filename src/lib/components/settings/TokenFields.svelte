<script lang="ts">
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import HelpPopover from '../common/HelpPopover.svelte';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  // Scopes and gateway policy of a named token (create and edit share it). Edits `draft` in
  // place; the parent validates with `policyOf` / `selectedScopes` before saving.
  import { t } from '#lib/i18n.svelte.js';
  import { Checkbox } from '#lib/components/ui/checkbox/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import { cn } from '#lib/utils.js';
  import {
    ENDPOINTS,
    SCOPES,
    impliedBy,
    limitsInvalid,
    noEndpoint,
    usesGateway,
    type TokenDraft,
  } from './tokenPolicy';

  interface Props {
    draft: TokenDraft;
    /** Unique prefix for element ids (several editors can be open). */
    idPrefix: string;
    /** The gateway accepts requests without a token (the policy only binds token holders). */
    gatewayOpen?: boolean;
  }

  let { draft = $bindable(), idPrefix, gatewayOpen = false }: Props = $props();

  const limitFields = [
    { key: 'rpm', label: 'dev.limit_rpm' },
    { key: 'tpm', label: 'dev.limit_tpm' },
    { key: 'concurrency', label: 'dev.limit_concurrency' },
  ] as const;
</script>

<div class="flex flex-col gap-3">
  <fieldset class="flex flex-col gap-1.5">
    <legend class="mb-1 text-xs font-medium text-muted-foreground">{t('dev.token_scopes')}</legend>
    <div class="grid grid-cols-2 gap-x-4 gap-y-1.5 max-[1100px]:grid-cols-1">
      {#each SCOPES as s (s.id)}
        {@const implied = impliedBy(draft, s.id)}
        <label
          class={cn('flex min-w-0 items-center gap-2 text-xs', implied && 'text-muted-foreground')}
          title={implied ? t('dev.scope_implied') : undefined}
        >
          {#if implied}
            <Checkbox checked disabled />
          {:else}
            <Checkbox bind:checked={draft.scopes[s.id]} />
          {/if}
          <span class="truncate font-mono">{t(s.label)}</span>
        </label>
      {/each}
    </div>
  </fieldset>

  {#if usesGateway(draft)}
    <div class="flex flex-col gap-3 rounded-md border border-border bg-muted/30 p-3">
      <div class="flex flex-col gap-0.5">
        <p class="text-xs font-medium">{t('dev.policy_title')}</p>
        {#if uiLevel.advanced}<HelpPopover text={t('dev.policy_hint')} />{:else}<p class="text-xs text-muted-foreground">{t('dev.policy_hint')}</p>{/if}
      </div>

      <div class="flex flex-col gap-1">
        <label for="{idPrefix}-models" class="text-xs font-medium">{t('dev.policy_models')}</label>
        <FieldHelp text={t('help.token_models')}><Textarea
          id="{idPrefix}-models"
          rows={2}
          class="min-h-0 font-mono text-xs"
          spellcheck={false}
          autocomplete="off"
          placeholder="qwen3-*"
          bind:value={draft.models}
        /></FieldHelp>
        {#if uiLevel.advanced}<HelpPopover text={t('dev.policy_models_hint')} />{:else}<p class="text-xs text-muted-foreground">{t('dev.policy_models_hint')}</p>{/if}
      </div>

      <fieldset class="flex flex-col gap-1.5">
        <legend class="mb-1 text-xs font-medium">{t('dev.policy_endpoints')}</legend>
        <div class="grid grid-cols-2 gap-x-4 gap-y-1.5 max-[1100px]:grid-cols-1">
          {#each ENDPOINTS as e (e.id)}
            <label class="flex min-w-0 items-center gap-2 text-xs">
              <FieldHelp text={t('help.token_endpoint')}><Checkbox bind:checked={draft.endpoints[e.id]} /></FieldHelp>
              <span class="truncate">{t(e.label)}</span>
            </label>
          {/each}
        </div>
        {#if noEndpoint(draft)}
          <p class="text-xs text-destructive" role="alert">{t('dev.policy_endpoints_none')}</p>
        {:else}
          {#if uiLevel.advanced}<HelpPopover text={t('dev.policy_endpoints_hint')} />{:else}<p class="text-xs text-muted-foreground">{t('dev.policy_endpoints_hint')}</p>{/if}
        {/if}
      </fieldset>

      <fieldset class="flex flex-col gap-1.5">
        <legend class="mb-1 text-xs font-medium">{t('dev.limits')}</legend>
        <div class="flex flex-wrap gap-3">
          {#each limitFields as f (f.key)}
            <div class="flex flex-col gap-1">
              <label for="{idPrefix}-{f.key}" class="text-xs text-muted-foreground">{t(f.label)}</label>
              <FieldHelp text={t('help.token_limits')}><Input
                id="{idPrefix}-{f.key}"
                inputmode="numeric"
                class="h-8 w-32 text-right font-num text-xs"
                placeholder={t('dev.limit_none')}
                autocomplete="off"
                bind:value={draft[f.key]}
              /></FieldHelp>
            </div>
          {/each}
        </div>
        {#if limitsInvalid(draft)}
          <p class="text-xs text-destructive" role="alert">{t('dev.limit_invalid')}</p>
        {:else}
          {#if uiLevel.advanced}<HelpPopover text={t('dev.limits_hint')} />{:else}<p class="text-xs text-muted-foreground">{t('dev.limits_hint')}</p>{/if}
        {/if}
      </fieldset>

      {#if gatewayOpen}
        <p class="text-xs text-warning" role="note">{t('dev.policy_open_note')}</p>
      {/if}
    </div>
  {/if}
</div>
