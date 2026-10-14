<script lang="ts">
  import { Input } from '#lib/components/ui/input/index.js';

  interface Props {
    id: string;
    value: number;
    onchange: (v: number) => void;
    min?: number;
    max?: number;
    /** 0 means automatic: shown as this placeholder in an empty field; clearing the field sets 0. */
    autoLabel?: string;
  }

  let { id, value, onchange, min, max, autoLabel }: Props = $props();

  /** The typed text is out of range (or empty): marked, not passed on. */
  let invalid = $state(false);

  const inRange = (v: number) => (autoLabel !== undefined && Number.isNaN(v)) || (!Number.isNaN(v) && (min === undefined || v >= min) && (max === undefined || v <= max));
</script>

<Input
  {id}
  type="number"
  {min}
  {max}
  class="h-8 w-28 text-right font-num"
  aria-invalid={invalid || undefined}
  placeholder={autoLabel}
  value={autoLabel !== undefined && value === 0 ? '' : value}
  oninput={(e) => {
    const v = e.currentTarget.valueAsNumber;
    invalid = !inRange(v);
    if (!invalid) onchange(Number.isNaN(v) ? 0 : v);
  }}
  onblur={(e) => {
    // An emptied or out-of-range field falls back to the last valid value instead of staying
    // blank or reaching the backend (a port above 65535 failed there with a raw type error).
    if (!inRange(e.currentTarget.valueAsNumber)) e.currentTarget.value = autoLabel !== undefined && value === 0 ? '' : String(value);
    invalid = false;
  }}
/>
