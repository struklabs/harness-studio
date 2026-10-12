<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import { Input } from '$lib/components/ui/input';
  import { Button } from '$lib/components/ui/button';
  import type { LaunchParamsForm } from '../params';
  import Field from '../parts/Field.svelte';
  import AdvSection from './AdvSection.svelte';
  import CatalogFields from './CatalogFields.svelte';

  interface Props {
    params: LaunchParamsForm;
    host: string;
    exposureRisk: boolean;
  }

  let { params = $bindable(), host, exposureRisk }: Props = $props();

  const uid = $props.id();
</script>

<AdvSection open={true} title={t('dash.n_sec_network')} badge={exposureRisk ? t('dash.n_badge_no_key') : undefined}>
  <Field label={t('dash.n_host_label')} id="{uid}-host">
    <Input id="{uid}-host" value={host} readonly class="font-num text-muted-foreground" />
  </Field>
  <Field help={t('help.server_key')} label={t('dash.adv_api_key_label')} id="{uid}-key" flag="--api-key">
    <Input
      id="{uid}-key"
      type="password"
      placeholder={params.api_key_ref ? t('dash.adv_api_key_saved_placeholder') : t('dash.adv_api_key_placeholder')}
      autocomplete="off"
      bind:value={params.api_key}
    />
    {#if params.api_key_ref}
      <Button variant="ghost" size="xs" class="self-start" onclick={() => (params.api_key_ref = null)}>
        {t('dash.adv_api_key_clear')}
      </Button>
    {/if}
  </Field>
  <CatalogFields bind:params group="server" />
</AdvSection>
