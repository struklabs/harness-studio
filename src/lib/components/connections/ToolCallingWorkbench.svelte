<script lang="ts">
  import HelpPopover from '../common/HelpPopover.svelte';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  import { onDestroy, onMount } from 'svelte';
  import { Calculator, CloudSun, FileText, Loader, Play, Wrench } from '@lucide/svelte';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import * as Collapsible from '#lib/components/ui/collapsible/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import * as ToggleGroup from '#lib/components/ui/toggle-group/index.js';
  import { t } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { cn } from '#lib/utils.js';
  import { fetchErrorText, fetchWithTimeout } from './net';
  import { streamInto } from '../playground/split';
  import type { ChatMessage } from '../playground/types';
  import MessageContent from '../playground/MessageContent.svelte';
  import { cancelCoreTool, coreToolDecision, listCoreTools, newRunId, runCoreTool, type CoreTool } from '#lib/tools-client.js';
  import { normalizeWorkbenchCall, realWorkbenchCall, runnableWorkbenchTool, sampleToolResult, toolRoundMessages, type WorkbenchCall } from './toolRound';

  interface Props {
    modelId: string;
  }

  let { modelId }: Props = $props();

  type Preset = 'weather' | 'calc' | 'read_file' | 'custom';

  const PRESETS: Record<Exclude<Preset, 'custom'>, { prompt: string; tool: unknown }> = {
    weather: {
      prompt: 'What is the current weather in Istanbul?',
      tool: {
        type: 'function',
        function: {
          name: 'get_current_weather',
          description: 'Get the current weather in a given location',
          parameters: {
            type: 'object',
            properties: {
              location: { type: 'string', description: 'City and state/country' },
              unit: { type: 'string', enum: ['celsius', 'fahrenheit'] },
            },
            required: ['location'],
          },
        },
      },
    },
    calc: {
      prompt: 'Calculate (144 * 12) + 256 for me.',
      tool: {
        type: 'function',
        function: {
          name: 'calculate_expression',
          description: 'Evaluate a mathematical expression accurately',
          parameters: {
            type: 'object',
            properties: {
              expression: { type: 'string', description: 'Mathematical expression to evaluate' },
            },
            required: ['expression'],
          },
        },
      },
    },
    read_file: {
      prompt: 'Please read the content of C:/projects/config.json.',
      tool: {
        type: 'function',
        function: {
          name: 'read_file',
          description: 'Read a local file and return its contents',
          parameters: {
            type: 'object',
            properties: {
              filepath: { type: 'string', description: 'Absolute path of file to read' },
            },
            required: ['filepath'],
          },
        },
      },
    },
  };

  interface TestResult {
    status: 'success_tool' | 'text_response' | 'error';
    latencyMs?: number;
    toolCall?: { name: string; arguments: unknown };
    textContent?: string;
    rawResponse?: string;
    errorMessage?: string;
  }

  let preset = $state<Preset>('weather');
  let prompt = $state(PRESETS.weather.prompt);
  let schema = $state(JSON.stringify(PRESETS.weather.tool, null, 2));
  let testing = $state(false);
  let result = $state<TestResult | null>(null);
  let rawOpen = $state(false);
  let controller: AbortController | null = null;
  let tools = $state<CoreTool[]>([]);
  let toolResult = $state('');
  let resultSource = $state<'sample' | 'real'>('sample');
  let finalMessage = $state<ChatMessage | null>(null);
  let continuing = $state(false);
  let roundError = $state('');
  let approvalPending = $state(false);
  let toolLatency = $state<number | null>(null);
  let round: { prompt: string; model: string; baseUrl: string; headers: Record<string, string>; assistant: { content?: string | null; reasoning_content?: string; tool_calls: WorkbenchCall[] } } | null = null;
  let activeRun: string | null = null;
  let realTool = $derived(result?.toolCall ? runnableWorkbenchTool(result.toolCall.name === 'get_current_weather' ? 'weather' : result.toolCall.name, tools) : undefined);
  let busy = $derived(testing || continuing);

  onMount(() => { void listCoreTools().then(listing => { tools = listing.tools; }).catch(() => {}); });

  function stop() {
    controller?.abort();
    if (continuing) roundError = t('conn.tool_round_stopped');
    approvalPending = false;
    if (activeRun) void cancelCoreTool(activeRun).catch(() => {});
  }

  async function executeRealTool(approved = false) {
    if (!result?.toolCall || continuing) return;
    roundError = '';
    finalMessage = null;
    continuing = true;
    const c = new AbortController();
    controller = c;
    try {
      let call: ReturnType<typeof realWorkbenchCall>;
      try { call = realWorkbenchCall(result.toolCall.name, result.toolCall.arguments); }
      catch { throw new Error(t('conn.tool_arguments_invalid')); }
      const listing = await listCoreTools();
      tools = listing.tools;
      if (c.signal.aborted) return;
      if (!runnableWorkbenchTool(call.name, tools)) throw new Error(t('conn.tool_real_excluded'));
      const decision = await coreToolDecision(call.name);
      if (c.signal.aborted) return;
      if (!decision || decision === 'refuse') throw new Error(t('conn.tool_real_refused'));
      if (decision === 'ask' && !approved) { approvalPending = true; return; }
      approvalPending = false;
      activeRun = newRunId();
      const output = await runCoreTool({ call_id: activeRun, tool: call.name, args: call.args, model: round?.model, approved: decision === 'ask' && approved });
      if (c.signal.aborted) return;
      toolResult = output.content;
      resultSource = 'real';
      toolLatency = output.duration_ms;
      if (output.error) throw new Error(output.content);
    } catch (e) {
      if (!c.signal.aborted) roundError = e instanceof Error ? e.message : String(e);
    } finally {
      activeRun = null;
      if (controller === c) { controller = null; continuing = false; }
    }
  }

  async function generateAnswer() {
    if (!round || !toolResult.trim() || busy) return;
    continuing = true;
    roundError = '';
    approvalPending = false;
    finalMessage = { id: newRunId(), role: 'assistant', content: '', streaming: true };
    const msg = finalMessage;
    const c = new AbortController();
    controller = c;
    const request = round;
    const requestUrl = `${request.baseUrl}/chat/completions`;
    const timeout = setTimeout(() => c.abort(), 120000);
    try {
      await streamInto(msg, { baseUrl: request.baseUrl, headers: request.headers, model: request.model, kind: 'server' }, {
        model: request.model,
        messages: toolRoundMessages(request.prompt, request.assistant, toolResult, resultSource),
        temperature: 0.1,
      }, c.signal, {
        http: (status, message) => t('pg.error_http', { status, message }),
        stream: (message) => t('pg.error_stream', { message }),
        network: (message) => t('pg.error_network', { message: fetchErrorText(new TypeError(message), requestUrl) }),
        noBody: t('pg.error_no_body'),
        toolLoop: t('pg.tool_loop_cut'),
      });
      if (c.signal.aborted) roundError = t('conn.tool_round_stopped');
      else if (!msg.error && (!msg.content.trim() || msg.toolCalls?.length)) roundError = t('conn.tool_unexpected_format');
    } catch (e) {
      if (!c.signal.aborted) roundError = fetchErrorText(e, requestUrl);
    } finally {
      clearTimeout(timeout);
      if (controller === c) { controller = null; continuing = false; }
    }
  }

  let isRunning = $derived(app.isRunning);

  onDestroy(stop);

  function selectPreset(value: string | undefined) {
    // ToggleGroup reports '' when the active item is clicked again; keep the current selection.
    if (!value || value === preset) return;
    preset = value as Preset;
    if (preset !== 'custom') {
      prompt = PRESETS[preset].prompt;
      schema = JSON.stringify(PRESETS[preset].tool, null, 2);
    }
  }

  async function runTest() {
    if (busy) return;
    if (!isRunning) {
      result = { status: 'error', errorMessage: t('conn.tool_server_not_running') };
      return;
    }
    controller?.abort();
    const c = new AbortController();
    controller = c;
    testing = true;
    result = null;
    round = null;
    finalMessage = null;
    toolResult = '';
    toolLatency = null;
    roundError = '';
    approvalPending = false;
    resultSource = 'sample';
    const request = { prompt, model: modelId, baseUrl: app.baseUrl, headers: app.authHeaders(), preset };
    const requestUrl = `${request.baseUrl}/chat/completions`;
    const started = performance.now();
    try {
      let parsedTool: unknown;
      try {
        parsedTool = JSON.parse(schema);
      } catch (e) {
        throw new Error(t('conn.tool_schema_invalid', { message: e instanceof Error ? e.message : String(e) }));
      }

      const res = await fetchWithTimeout(
        requestUrl,
        {
          method: 'POST',
          headers: { 'Content-Type': 'application/json', ...request.headers },
          body: JSON.stringify({
            model: request.model,
            messages: [{ role: 'user', content: request.prompt }],
            tools: [parsedTool],
            tool_choice: 'auto',
            temperature: 0.1,
          }),
          signal: c.signal,
        },
        120000
      );
      const elapsed = Math.round(performance.now() - started);

      if (!res.ok) {
        const errText = await res.text();
        throw new Error(t('conn.tool_http_error', { status: res.status, errText }));
      }

      const data = await res.json();
      if (c.signal.aborted) return;
      const message = data.choices?.[0]?.message;
      const raw = JSON.stringify(data, null, 2);

      if (message?.tool_calls?.length > 0) {
        if (message.tool_calls.length !== 1) throw new Error(t('conn.tool_single_call_only'));
        let first: WorkbenchCall;
        try { first = normalizeWorkbenchCall(message.tool_calls[0]); }
        catch { throw new Error(t('conn.tool_unexpected_format')); }
        round = { ...request, assistant: { content: message.content ?? null, ...(message.reasoning_content ? { reasoning_content: message.reasoning_content } : {}), tool_calls: [first] } };
        let args: unknown = first.function?.arguments;
        if (typeof args === 'string') {
          try {
            args = JSON.parse(args);
          } catch {
            /* keep the raw string */
          }
        }
        toolResult = sampleToolResult(request.preset, args);
        result = {
          status: 'success_tool',
          latencyMs: elapsed,
          toolCall: { name: first.function?.name || 'unknown', arguments: args },
          rawResponse: raw,
        };
      } else if (message?.content) {
        result = { status: 'text_response', latencyMs: elapsed, textContent: message.content, rawResponse: raw };
      } else {
        result = { status: 'error', latencyMs: elapsed, errorMessage: t('conn.tool_unexpected_format'), rawResponse: raw };
      }
    } catch (e) {
      if (!c.signal.aborted) result = { status: 'error', errorMessage: fetchErrorText(e, requestUrl) };
    } finally {
      if (controller === c) {
        controller = null;
        testing = false;
      }
    }
  }

  const statusLabel = (s: TestResult['status']) =>
    s === 'success_tool' ? t('conn.status_tool_call_success') : s === 'text_response' ? t('conn.status_text_response') : t('conn.status_error');
  const statusTone = (s: TestResult['status']) =>
    s === 'success_tool' ? 'text-success border-success/40' : s === 'text_response' ? 'text-warning border-warning/40' : 'text-destructive border-destructive/40';
</script>

<section class="flex min-w-0 flex-col gap-3" aria-labelledby="conn-test-title">
  <div class="flex items-center justify-between gap-3">
    <div class="flex min-w-0 flex-col">
      <h2 id="conn-test-title" class="text-base font-medium">{t('conn.tool_calling_title')}<HelpPopover text={t('conn.tool_desc')} /></h2>
    </div>
    <span class={cn('flex shrink-0 items-center gap-1.5 text-xs', isRunning ? 'text-success' : 'text-muted-foreground')}>
      <span class={cn('size-1.5 rounded-full', isRunning ? 'bg-success' : 'bg-muted-foreground/50')}></span>
      {isRunning ? t('conn.tool_status_ready') : t('conn.tool_status_off')}
    </span>
  </div>

  <div class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-4 max-[1100px]:grid-cols-1">
    <!-- Input -->
    <div class="flex min-w-0 flex-col gap-3 rounded-lg border border-border bg-card p-3">
      <div class="flex flex-col gap-1.5">
        <span class="text-xs font-medium text-muted-foreground">{t('conn.tool_preset_label')}</span>
        <FieldHelp text={t('help.work_preset')}><ToggleGroup.Root
          type="single"
          variant="outline"
          size="sm"
          value={preset}
          onValueChange={selectPreset}
          class="flex-wrap"
          aria-label={t('conn.tool_preset_label')}
        >
          <ToggleGroup.Item value="weather"><CloudSun /> <span class="font-mono text-xs">get_current_weather</span></ToggleGroup.Item>
          <ToggleGroup.Item value="calc"><Calculator /> <span class="font-mono text-xs">calculate_expression</span></ToggleGroup.Item>
          <ToggleGroup.Item value="read_file"><FileText /> <span class="font-mono text-xs">read_file</span></ToggleGroup.Item>
          <ToggleGroup.Item value="custom">{t('conn.custom_json_schema')}</ToggleGroup.Item>
        </ToggleGroup.Root></FieldHelp>
      </div>

      <div class="flex flex-col gap-1.5">
        <Label for="conn-tool-prompt" class="text-xs text-muted-foreground">{t('conn.test_prompt_label')}</Label>
        <div class="flex items-center gap-2">
          <FieldHelp text={t('help.work_prompt')}><Input
            id="conn-tool-prompt"
            bind:value={prompt}
            placeholder={t('conn.test_prompt_placeholder')}
            class="h-8 min-w-0 flex-1"
            onkeydown={(e) => {
              if (e.key === 'Enter' && isRunning && prompt.trim()) runTest();
            }}
          /></FieldHelp>
          <Button onclick={runTest} disabled={busy || !isRunning || !prompt.trim()}>
            {#if testing}<Loader class="animate-spin" />{:else}<Play />{/if}
            {testing ? t('conn.running_ellipsis') : t('conn.run_test')}
          </Button>
        </div>
      </div>

      <div class="flex flex-col gap-1.5">
        <Label for="conn-tool-schema" class="text-xs text-muted-foreground">{t('conn.tool_def_label')}</Label>
        <FieldHelp text={t('help.work_schema')}><Textarea
          id="conn-tool-schema"
          bind:value={schema}
          rows={10}
          spellcheck="false"
          class="max-h-72 min-h-40 font-mono text-xs"
          oninput={() => (preset = 'custom')}
        /></FieldHelp>
      </div>
    </div>

    <!-- Result -->
    <div class="flex min-w-0 flex-col gap-2 rounded-lg border border-border bg-card p-3" aria-live="polite">
      <div class="flex items-center justify-between gap-2">
        <span class="text-sm font-semibold text-foreground">{t('conn.verification_result_label')}</span>
        {#if result}
          <Badge variant="outline" class={statusTone(result.status)}>{statusLabel(result.status)}</Badge>
        {/if}
      </div>

      {#if testing}
        <div class="flex items-center gap-2 py-8 text-xs text-muted-foreground">
          <Loader class="size-4 animate-spin" />
          <span>{t('conn.waiting_response')}</span>
        </div>
      {:else if result}
        {#if result.status === 'success_tool' && result.toolCall}
          <div class="flex flex-col gap-2">
            <div class="flex items-center justify-between gap-2 text-xs">
              <span class="min-w-0 truncate">
                {t('conn.called_function_label')} <code class="font-mono text-foreground">{result.toolCall.name}</code>
              </span>
              {#if result.latencyMs}
                <span class="font-num text-muted-foreground">{t('conn.latency_ms', { ms: result.latencyMs })}</span>
              {/if}
            </div>
            <span class="text-xs text-muted-foreground">{t('conn.generated_json_params_label')}</span>
            <pre class="max-h-64 overflow-auto rounded-md border border-border bg-muted p-3 font-mono text-xs whitespace-pre-wrap break-all"><code>{JSON.stringify(result.toolCall.arguments, null, 2)}</code></pre>
            <Label for="conn-tool-result" class="text-xs text-muted-foreground">{t(resultSource === 'real' ? 'conn.tool_result_real' : 'conn.tool_result_sample')}</Label>
            {#if resultSource === 'sample'}<p class="text-xs text-muted-foreground">{t('conn.tool_sample_note')}</p>{/if}
            <FieldHelp text={t('help.work_result')}><Textarea id="conn-tool-result" bind:value={toolResult} disabled={continuing} rows={5} spellcheck="false" class="max-h-64 font-mono text-xs" oninput={() => { resultSource = 'sample'; toolLatency = null; finalMessage = null; }} /></FieldHelp>
            {#if toolLatency !== null}<span class="font-num text-xs text-muted-foreground">{t('conn.latency_ms', { ms: toolLatency })}</span>{/if}
            <div class="flex flex-wrap gap-2">
              {#if realTool}
                <Button variant="outline" disabled={busy} onclick={() => executeRealTool()}>{t('conn.tool_use_real', { name: realTool.name })}</Button>
              {/if}
              <Button disabled={busy || !toolResult.trim()} onclick={generateAnswer}>{t('conn.tool_generate_answer')}</Button>
              {#if continuing}<Button variant="outline" onclick={stop}>{t('conn.tool_stop')}</Button>{/if}
            </div>
            {#if approvalPending && realTool}
              <div class="rounded-md border border-warning/40 p-3 text-xs">
                <p>{t('conn.tool_approval_note', { name: realTool.name })}</p>
                <pre class="mt-2 overflow-auto font-mono whitespace-pre-wrap">{JSON.stringify(realWorkbenchCall(result.toolCall.name, result.toolCall.arguments).args, null, 2)}</pre>
                <div class="mt-2 flex gap-2">
                  <Button disabled={busy} onclick={() => executeRealTool(true)}>{t('pg.tool_approve')}</Button>
                  <Button variant="outline" disabled={busy} onclick={() => { approvalPending = false; }}>{t('pg.tool_deny')}</Button>
                </div>
              </div>
            {/if}
            {#if continuing}<span class="flex items-center gap-2 text-xs text-muted-foreground"><Loader class="size-4 animate-spin" />{t('conn.waiting_response')}</span>{/if}
            {#if finalMessage}
              <span class="text-sm font-semibold">{t('conn.tool_final_answer')}</span>
              <div data-testid="workbench-final-answer" class="max-h-80 overflow-auto rounded-md border border-border bg-muted p-3"><MessageContent text={finalMessage.content} /></div>
              {#if finalMessage.stats?.totalMs !== undefined}<span class="font-num text-xs text-muted-foreground">{t('conn.latency_ms', { ms: Math.round(finalMessage.stats.totalMs) })}</span>{/if}
            {/if}
            {#if roundError || finalMessage?.error}<p class="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-xs text-destructive" role="alert">{roundError || finalMessage?.error}</p>{/if}
          </div>
        {:else if result.status === 'text_response'}
          <div class="flex flex-col gap-2">
            <p class="text-xs text-muted-foreground">{t('conn.text_response_note')}</p>
            <p class="max-h-64 overflow-auto rounded-md border border-border bg-muted p-3 text-xs whitespace-pre-wrap break-words">{result.textContent}</p>
            {#if result.latencyMs}
              <span class="font-num text-xs text-muted-foreground">{t('conn.latency_ms', { ms: result.latencyMs })}</span>
            {/if}
          </div>
        {:else}
          <p class="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-xs break-words text-destructive" role="alert">
            <strong>{t('conn.error_label')}</strong> {result.errorMessage}
          </p>
        {/if}

        {#if result.rawResponse}
          <Collapsible.Root bind:open={rawOpen}>
            <Collapsible.Trigger class="text-xs text-muted-foreground underline-offset-4 hover:text-foreground hover:underline">
              {t('conn.raw_response_summary')}
            </Collapsible.Trigger>
            <Collapsible.Content>
              <pre class="mt-2 max-h-72 overflow-auto rounded-md border border-border bg-muted p-3 font-mono text-xs whitespace-pre-wrap break-all"><code>{result.rawResponse}</code></pre>
            </Collapsible.Content>
          </Collapsible.Root>
        {/if}
      {:else}
        <div class="flex flex-1 flex-col items-center justify-center gap-2 py-10 text-center text-xs text-muted-foreground">
          <Wrench class="size-7 opacity-50" />
          <p class="max-w-64">{t('conn.placeholder_text')}</p>
        </div>
      {/if}
    </div>
  </div>
</section>
