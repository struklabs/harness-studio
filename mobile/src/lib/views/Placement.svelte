<script lang="ts">
  import Cpu from '@lucide/svelte/icons/cpu';
  import Cloud from '@lucide/svelte/icons/cloud';
  import { t } from '#lib/i18n.svelte.js';
  import type { Placement } from '../harness.ts';
  import type { ProviderModel, Usage } from '../providers.ts';
  import { usd } from '../text.ts';

  let { placement, model = null, usage = null, cost = null }: { placement: Placement; model?: ProviderModel | null; usage?: Usage | null; cost?: number | null } = $props();
</script>

<!-- Where this reply ran: a provider's cloud (the phone is only the client) or a Harness
     server and the device it uses. -->
<div class="mt-1 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-muted-foreground" data-testid="placement">
  {#if placement.where !== 'harness'}
    <Cloud class="size-3" />
    <span>{placement.where === 'endpoint' ? t('mobile.where.endpoint', { name: placement.provider }) : t('mobile.where.provider', { provider: placement.provider })}</span>
    <span>· {t('mobile.where.this_phone_client')}</span>
  {:else}
    <Cpu class="size-3" />
    <span>{t('mobile.where.harness', { name: placement.provider })}</span>
    {#if placement.server}<span>· {t('mobile.where.server', { id: placement.server })}</span>{/if}
    <span>· {placement.device ? t('mobile.where.device', { device: placement.device }) : t('mobile.where.device_unknown')}</span>
    {#if placement.backend}<span>({placement.backend})</span>{/if}
    {#if placement.queueMs !== null}<span>· {t('mobile.where.queue', { ms: placement.queueMs })}</span>{/if}
    {#if placement.loadMs}<span>· {t('mobile.where.load', { ms: placement.loadMs })}</span>{/if}
  {/if}
  {#if placement.model}<span>· {placement.model}</span>{/if}
  {#if model?.contextLength}<span>· {t('mobile.cap.context', { n: model.contextLength.toLocaleString() })}</span>{/if}
  {#if model?.tools}<span>· {t('mobile.cap.tools')}</span>{/if}
  {#if model?.vision}<span>· {t('mobile.cap.vision')}</span>{/if}
  {#if usage}<span>· {t('mobile.usage', { prompt: usage.promptTokens, completion: usage.completionTokens })}</span>{/if}
  {#if cost !== null}<span>· {usd(cost)}</span>{/if}
</div>
