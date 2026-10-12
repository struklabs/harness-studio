<script lang="ts">
  import { Switch } from '$lib/components/ui/switch';
  import SettingRow from './SettingRow.svelte';

  interface Props {
    id: string;
    label: string;
    hint?: string;
    note?: string;
    help?: string;
    flag?: string;
    advanced?: boolean;
    checked: boolean;
    onchange: (v: boolean) => void;
    disabled?: boolean;
    dirty?: boolean;
  }

  let { id, label, hint, note, help, flag, advanced = false, checked, onchange, disabled = false, dirty = false }: Props = $props();
</script>

<SettingRow {label} {hint} {note} {help} {flag} {advanced} for={id} {dirty}>
  <Switch {id} {disabled} bind:checked={() => checked, (v) => onchange(v)} />
</SettingRow>
