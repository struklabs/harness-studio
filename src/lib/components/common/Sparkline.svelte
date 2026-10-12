<script lang="ts">
  interface Props {
    values: number[];
    width?: number;
    height?: number;
    class?: string;
    /** Line colour (CSS value); defaults to the text colour from `class`. */
    color?: string;
  }

  let { values, width = 56, height = 18, class: className = '', color }: Props = $props();

  let path = $derived.by(() => {
    if (values.length < 2) return '';
    const max = Math.max(...values, 1);
    const step = width / (values.length - 1);
    return values
      .map((v, i) => `${i === 0 ? 'M' : 'L'}${(i * step).toFixed(1)},${(height - 1 - (v / max) * (height - 3)).toFixed(1)}`)
      .join(' ');
  });
</script>

<svg {width} {height} viewBox="0 0 {width} {height}" class={className} style:color={color} aria-hidden="true">
  {#if path}
    <path d={path} fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" stroke-linecap="round" />
  {:else}
    <line x1="0" y1={height - 1} x2={width} y2={height - 1} stroke="currentColor" stroke-opacity="0.3" stroke-dasharray="2 3" />
  {/if}
</svg>
