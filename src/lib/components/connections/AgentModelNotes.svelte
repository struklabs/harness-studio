<script lang="ts">
  import AdvancedOnly from '../common/AdvancedOnly.svelte';

  // What an agent tool can expect from one model: native tool calls (verified by a real answer,
  // the gateway log or only the chat template), a better model when tool calls fail, and how
  // long the first answer waits for the tool's big system prompt.
  import { FlaskConical, TriangleAlert } from '@lucide/svelte';
  import { Button } from '#lib/components/ui/button/index.js';
  import { locale, t } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { rawError } from '#lib/errors.js';
  import { cn } from '#lib/utils.js';
  import { security } from '#lib/stores/security.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { samePath } from '#lib/stores/domain.svelte.js';
  import {
    checkToolCalling,
    findAgentModel,
    firstReplySeconds,
    FIRST_REPLY_WARN_SECONDS,
    promptSpeed,
    toolSuggestions,
    toolWarning,
    TOOL_MIN_ROUNDS,
    type AgentModel,
  } from './agentModels';

  interface Props {
    alias: string;
    /** Every agent model (for the verdict and suggestions). */
    models: AgentModel[];
    toolName: string;
    /** Tokens the tool sends before the first answer (null = no first-reply note). */
    promptTokens?: number | null;
    /** Endpoint for the tool check (`http://host:port/v1`); null = no check button. */
    checkUrl: string | null;
    checkKey: string | null;
    disabled?: boolean;
    /** Switch to a suggested model (null = suggestions are plain text). */
    onPick?: ((alias: string) => void) | null;
    /** A check finished: the parent reloads the models. */
    onChecked: () => void;
  }

  let { alias, models, toolName, promptTokens = null, checkUrl, checkKey, disabled = false, onPick = null, onChecked }: Props = $props();

  let checking = $state(false);
  let error = $state('');
  let model = $derived(findAgentModel(models, alias));
  let warn = $derived(toolWarning(model));
  let suggestions = $derived(warn ? toolSuggestions(models, model?.alias ?? alias) : []);
  // Average prompt speed of a running server of this model (live telemetry), the last resort
  // when neither the gateway log nor a benchmark has measured it.
  let liveTps = $derived.by(() => {
    const path = model?.model_path;
    if (!path) return null;
    const server = app.servers.find((s) => s.active && samePath(s.active.model_path, path));
    return server ? (app.telemetryAll[server.id]?.avg_prompt_tokens_per_sec ?? null) : null;
  });
  let speed = $derived(promptTokens ? promptSpeed(model, liveTps) : null);
  let seconds = $derived(promptTokens && speed ? firstReplySeconds(speed, promptTokens) : null);
  let slow = $derived(seconds !== null && seconds > FIRST_REPLY_WARN_SECONDS);

  $effect(() => {
    void alias;
    error = '';
  });

  // Tool-calling level learned from the gateway's diagnosed tool rounds (T4).
  let rounds = $derived((model?.tool_rounds_ok ?? 0) + (model?.tool_rounds_bad ?? 0));
  let levelText = $derived(
    model
      ? model.tool_level
        ? t(`td.level_${model.tool_level}`, { ok: model.tool_rounds_ok, n: rounds })
        : t('td.level_unmeasured', { n: rounds, need: TOOL_MIN_ROUNDS })
      : ''
  );

  function when(ts: number): string {
    return new Date(ts * 1000).toLocaleString(locale(), { dateStyle: 'short', timeStyle: 'short' });
  }

  function statusText(m: AgentModel | undefined): string {
    const name = m?.alias ?? alias;
    switch (m?.tools) {
      case 'verified':
        return m.evidence === 'check' && m.check
          ? t('agent.tools_verified_check', { alias: name, when: when(m.check.checked_at) })
          : t('agent.tools_verified_log', { alias: name, count: m.native_tool_calls });
      case 'failed':
        return m.check?.text_call ? t('agent.tools_failed_text', { alias: name, tool: toolName }) : t('agent.tools_failed', { alias: name, tool: toolName });
      case 'no_template':
        return t('agent.tools_no_template', { alias: name, tool: toolName });
      case 'template_only':
        return t('agent.tools_template_only');
      default:
        return t('agent.tools_unknown');
    }
  }

  async function runCheck() {
    if (!checkUrl || checking) return;
    checking = true;
    error = '';
    try {
      await checkToolCalling(checkUrl, checkKey, model?.alias ?? alias);
      onChecked();
    } catch (e) {
      const msg = rawError(e);
      error = localizeError(msg) || msg;
    } finally {
      checking = false;
    }
  }
</script>

<div class="flex flex-col gap-2 text-xs">
  <div
    class={cn(
      'flex items-start gap-2 rounded-md border px-3 py-2',
      warn ? 'border-warning/40 bg-warning/5 text-foreground' : 'border-border text-muted-foreground'
    )}
    role="status"
  >
    {#if warn}<TriangleAlert class="mt-0.5 size-3.5 shrink-0 text-warning" />{/if}
    <div class="flex min-w-0 flex-1 flex-col gap-2">
      <span class={cn('break-words', model?.tools === 'verified' && 'text-success')}>{statusText(model)}</span>
      {#if model}
        <span class={cn('break-words', model.tool_level === 'limited' && 'text-warning', model.tool_level === 'native' && 'text-success')}>{levelText}</span>
        <span class="break-words text-muted-foreground">{t('td.recommended', { n: model.recommended_tools })}</span>
      {/if}
      {#if model?.tool_level === 'limited'}
        <Button size="sm" variant="outline" class="self-start" onclick={() => security.openSettings('gateway-routing')} title={t('gw.text_calls_desc')}>{t('gw.text_calls_hint')}</Button>
      {/if}
      {#if model?.check && !model.check.tool_called && model.check.detail}
        <code class="block max-h-20 overflow-auto rounded bg-muted px-2 py-1 font-mono break-all whitespace-pre-wrap text-muted-foreground">{localizeError(model.check.detail)}</code>
      {/if}
      {#if warn}
        {#if suggestions.length}
          <div class="flex flex-col gap-1">
            <span class="text-muted-foreground">{t('agent.suggest')}</span>
            <ul class="flex flex-col gap-1">
              {#each suggestions as s (s.alias)}
                <li class="flex min-w-0 flex-wrap items-center gap-2">
                  <span class="min-w-0 truncate font-mono">{s.alias}</span>
                  <span class={s.tools === 'verified' ? 'text-success' : 'text-muted-foreground'}>
                    {s.tools === 'verified' ? t('agent.suggest_verified') : t('agent.suggest_template')}
                  </span>
                  {#if onPick}
                    <Button size="sm" variant="outline" class="h-6 px-2 text-xs" onclick={() => onPick?.(s.alias)} {disabled}>{t('agent.suggest_use')}</Button>
                  {/if}
                </li>
              {/each}
            </ul>
          </div>
        {:else}
          <span class="text-muted-foreground">{t('agent.suggest_none')}</span>
        {/if}
      {/if}
      {#if checkUrl && model?.tools !== 'no_template'}
        <Button size="sm" variant="outline" class="self-start" onclick={runCheck} disabled={disabled || checking} title={t('agent.check_tip')}>
          <FlaskConical class={cn(checking && 'animate-pulse')} />
          {checking ? t('agent.checking') : model?.check ? t('agent.check_again') : t('agent.check')}
        </Button>
      {/if}
      {#if error}
        <span class="break-words text-destructive" role="alert">{error}</span>
      {/if}
    </div>
  </div>

  {#if promptTokens}
    <AdvancedOnly>
      <p class="text-muted-foreground">
        {seconds !== null && speed
          ? t('agent.first_reply_measured', {
              tool: toolName,
              tokens: promptTokens.toLocaleString(locale()),
              tps: Math.round(speed.tps).toLocaleString(locale()),
              source: t(`agent.speed_${speed.source}`),
              seconds: Math.max(1, Math.round(seconds)),
            })
          : t('agent.first_reply_static', { tool: toolName, tokens: promptTokens.toLocaleString(locale()) })}
      </p>
    </AdvancedOnly>
    {#if slow && seconds !== null}
      <div class="flex items-start gap-2 rounded-md border border-warning/40 bg-warning/5 px-3 py-2 text-foreground" role="status">
        <TriangleAlert class="mt-0.5 size-3.5 shrink-0 text-warning" />
        <span class="break-words">{t('agent.first_reply_slow', { tool: toolName, seconds: Math.round(seconds) })}</span>
      </div>
    {/if}
  {/if}
</div>
