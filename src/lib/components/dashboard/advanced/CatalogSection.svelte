<script lang="ts">
  import { t, type TKey } from '$lib/i18n.svelte';
  import { capsStore } from '$lib/stores/caps.svelte';
  import type { LaunchParamsForm } from '../params';
  import AdvSection from './AdvSection.svelte';
  import CatalogFields from './CatalogFields.svelte';

  interface Props {
    params: LaunchParamsForm;
    /** Catalog group rendered as its own section (params.toml `group`). */
    group: string;
  }

  let { params = $bindable(), group }: Props = $props();

  let hasAny = $derived(capsStore.groupSpecs(group).some((s) => capsStore.hasAny(s.flags)));
</script>

{#if hasAny}
  <AdvSection open={true} title={t(`caps.group.${group}` as TKey)}>
    <CatalogFields bind:params {group} />
  </AdvSection>
{/if}
