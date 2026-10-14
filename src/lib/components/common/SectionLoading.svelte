<script lang="ts">
  /**
   * Loading state of a section whose data is really still being fetched. Not a spinner: a
   * skeleton of the section's own shape, a slow light sweep across it, a small "neural" motif
   * (nodes with pulses travelling along edges) and a status line cycling through the phases
   * the section really goes through. After ~3 s it escalates subtly: a second sweep, brighter
   * pulses and an elapsed-time hint, so a long load still feels alive.
   *
   * Use it in place of the content while the section's own loading flag/promise is pending; it
   * is sized by `shape` / `rows` / `minHeight` to the content it replaces so the layout does not
   * jump. Animation stops under `prefers-reduced-motion` (see the CSS) and the phase text then
   * stays on the first phase.
   */
  import { onMount } from 'svelte';
  import { t } from '#lib/i18n.svelte.js';

  type Shape = 'cards' | 'rows' | 'chart' | 'panel' | 'page' | 'inline';

  interface Props {
    /** Short label for screen readers (a polite live region says it once). */
    label: string;
    /** Real phases of this section, already translated; cycled in order. */
    phases?: string[];
    /** Skeleton shape: `cards` KPI tiles + a block, `rows` a list, `chart` a plot, `panel` a
     *  generic block, `page` a whole view (tab bar + tiles + rows), `inline` only the status line. */
    shape?: Shape;
    /** Number of rows for `rows` / `page`. */
    rows?: number;
    /** CSS min-height of the skeleton (e.g. `12rem`), to match the content it replaces. */
    minHeight?: string;
    class?: string;
  }

  let { label, phases = [], shape = 'panel', rows = 4, minHeight, class: className = '' }: Props = $props();

  const PHASE_MS = 2400;
  const DEEP_AFTER_S = 3;

  let phaseIndex = $state(0);
  let elapsed = $state(0);
  let reduced = $state(false);
  let deep = $derived(elapsed >= DEEP_AFTER_S);
  let phase = $derived(phases.length ? phases[phaseIndex % phases.length] : label);

  onMount(() => {
    const started = Date.now();
    reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;
    // One cheap 1 s tick drives the elapsed hint; the phase advances every PHASE_MS of it.
    const tick = setInterval(() => {
      elapsed = Math.floor((Date.now() - started) / 1000);
      if (!reduced && phases.length > 1) phaseIndex = Math.floor((Date.now() - started) / PHASE_MS);
    }, 500);
    return () => clearInterval(tick);
  });

  const bars = [34, 58, 44, 72, 52, 86, 64, 78, 48, 66, 90, 56];
</script>

{#snippet motif(small: boolean)}
  <svg class="sl-motif" class:sl-small={small} viewBox="0 0 84 32" aria-hidden="true">
    <g class="sl-edges">
      <path d="M8 20 L26 8" />
      <path d="M26 8 L44 22" />
      <path d="M44 22 L60 8" />
      <path d="M60 8 L76 18" />
      <path d="M26 8 L60 8" />
    </g>
    <g class="sl-pulses">
      <path d="M8 20 L26 8" pathLength="100" style="animation-delay: 0s" />
      <path d="M26 8 L44 22" pathLength="100" style="animation-delay: 0.45s" />
      <path d="M44 22 L60 8" pathLength="100" style="animation-delay: 0.9s" />
      <path d="M60 8 L76 18" pathLength="100" style="animation-delay: 1.35s" />
      <path d="M26 8 L60 8" pathLength="100" style="animation-delay: 1.8s" />
    </g>
    <g class="sl-nodes">
      <circle cx="8" cy="20" r="2.4" style="animation-delay: 0s" />
      <circle cx="26" cy="8" r="2.8" style="animation-delay: 0.4s" />
      <circle cx="44" cy="22" r="2.6" style="animation-delay: 0.8s" />
      <circle cx="60" cy="8" r="2.8" style="animation-delay: 1.2s" />
      <circle cx="76" cy="18" r="2.4" style="animation-delay: 1.6s" />
    </g>
  </svg>
{/snippet}

{#snippet status(small: boolean)}
  <div class="sl-status" class:sl-status-small={small} aria-hidden="true">
    {@render motif(small)}
    <span class="sl-text">
      {#key phase}<span class="sl-phase">{phase}</span>{/key}
      {#if deep}<span class="sl-elapsed font-num">{t('sl.elapsed', { s: elapsed })}</span>{/if}
    </span>
  </div>
{/snippet}

<div
  class="sl-root sl-{shape} {className}"
  class:sl-deep={deep}
  style:min-height={shape === 'inline' ? undefined : minHeight}
  aria-busy="true"
>
  <div class="sr-only" role="status" aria-live="polite">{label}</div>

  {#if shape === 'inline'}
    {@render status(true)}
  {:else}
    <div class="sl-bones" aria-hidden="true">
      {#if shape === 'cards' || shape === 'page'}
        {#if shape === 'page'}
          <div class="sl-bar">
            <i class="sl-bone" style="width: 7.5rem; height: 1.75rem"></i>
            <i class="sl-bone" style="width: 7.5rem; height: 1.75rem"></i>
          </div>
        {/if}
        <div class="sl-tiles">
          {#each [0, 1, 2, 3] as n (n)}
            <div class="sl-tile">
              <i class="sl-bone" style="width: 45%; height: 0.55rem"></i>
              <i class="sl-bone" style="width: 62%; height: 1.4rem"></i>
              <i class="sl-bone" style="width: 35%; height: 0.5rem"></i>
            </div>
          {/each}
        </div>
      {/if}

      {#if shape === 'chart' || shape === 'cards'}
        <div class="sl-plot">
          {#each bars as h, n (n)}
            <i class="sl-bone sl-col" style="height: {h}%; animation-delay: {n * 90}ms"></i>
          {/each}
        </div>
      {/if}

      {#if shape === 'rows' || shape === 'page'}
        <div class="sl-rows">
          {#each Array.from({ length: rows }, (_, n) => n) as n (n)}
            <div class="sl-row">
              <i class="sl-bone" style="width: 1.5rem; height: 1.5rem; border-radius: 9999px"></i>
              <div class="sl-row-text">
                <i class="sl-bone" style="width: {62 - (n % 3) * 12}%; height: 0.7rem"></i>
                <i class="sl-bone" style="width: {38 + (n % 2) * 14}%; height: 0.5rem"></i>
              </div>
              <i class="sl-bone" style="width: 3.5rem; height: 1.25rem"></i>
            </div>
          {/each}
        </div>
      {/if}

      {#if shape === 'panel'}
        <i class="sl-bone" style="width: 40%; height: 0.8rem"></i>
        <i class="sl-bone" style="width: 100%; height: 0.55rem"></i>
        <i class="sl-bone" style="width: 86%; height: 0.55rem"></i>
        <i class="sl-bone sl-grow" style="width: 100%"></i>
      {/if}
    </div>
    <div class="sl-badge">{@render status(false)}</div>
  {/if}
</div>

<style>
  .sl-root {
    position: relative;
    overflow: hidden;
    border-radius: var(--radius-md, 0.5rem);
    /* Fade in after a beat: a load that finishes at once never flashes the skeleton. */
    animation: sl-in 220ms ease-out 140ms both;
  }
  .sl-page {
    height: 100%;
    padding: 1rem 1.5rem;
    border-radius: 0;
  }
  .sl-bones {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    height: 100%;
    min-height: inherit;
  }
  .sl-bone {
    display: block;
    border-radius: 0.375rem;
    background: color-mix(in oklab, var(--muted-foreground) 14%, var(--muted));
    opacity: 0.7;
  }
  .sl-bar {
    display: flex;
    gap: 0.5rem;
    padding-bottom: 0.5rem;
    border-bottom: 1px solid var(--border);
  }
  .sl-tiles {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(9rem, 1fr));
    gap: 0.75rem;
  }
  .sl-tile {
    display: flex;
    flex-direction: column;
    gap: 0.55rem;
    padding: 0.75rem;
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: color-mix(in oklab, var(--card) 70%, transparent);
  }
  .sl-plot {
    flex: 1;
    min-height: 7rem;
    display: flex;
    align-items: flex-end;
    gap: 0.4rem;
    padding: 0.75rem;
    border: 1px solid var(--border);
    border-radius: 0.5rem;
  }
  .sl-col {
    flex: 1;
    border-radius: 0.25rem 0.25rem 0 0;
    transform-origin: bottom;
    animation: sl-grow 2.4s ease-in-out infinite;
  }
  .sl-rows {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .sl-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.6rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: color-mix(in oklab, var(--card) 70%, transparent);
  }
  .sl-row-text {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  .sl-grow {
    flex: 1;
    min-height: 4rem;
  }

  /* The light sweep: one soft diagonal band across the whole section. */
  .sl-root:not(.sl-inline)::after,
  .sl-root:not(.sl-inline)::before {
    content: '';
    position: absolute;
    inset: 0;
    pointer-events: none;
    background: linear-gradient(
      105deg,
      transparent 38%,
      color-mix(in oklab, var(--primary) 8%, transparent) 50%,
      transparent 62%
    );
    transform: translateX(-100%);
    animation: sl-sweep 3.2s ease-in-out infinite;
  }
  /* Past ~3 s: a second, quicker band and a stronger first one. */
  .sl-root:not(.sl-inline)::before {
    opacity: 0;
    animation-duration: 2.1s;
    animation-delay: 0.9s;
    transition: opacity 600ms ease;
  }
  .sl-deep:not(.sl-inline)::before {
    opacity: 1;
  }
  .sl-deep:not(.sl-inline)::after {
    background: linear-gradient(
      105deg,
      transparent 34%,
      color-mix(in oklab, var(--primary) 13%, transparent) 50%,
      transparent 66%
    );
  }

  /* Status chip with the neural motif, centred over the skeleton. */
  .sl-badge {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
    z-index: 1; /* above the sweep bands */
  }
  .sl-badge .sl-status {
    padding: 0.4rem 0.75rem 0.4rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: 9999px;
    background: color-mix(in oklab, var(--background) 82%, transparent);
    backdrop-filter: blur(4px);
    box-shadow: 0 1px 8px color-mix(in oklab, var(--primary) 10%, transparent);
  }
  .sl-status {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    max-width: 100%;
    color: var(--muted-foreground);
    font-size: 0.75rem;
    line-height: 1;
  }
  .sl-status-small {
    font-size: 0.75rem;
    padding: 0.25rem 0;
  }
  .sl-text {
    display: inline-flex;
    align-items: baseline;
    gap: 0.5rem;
    min-width: 0;
  }
  .sl-phase {
    animation: sl-phase 380ms ease-out both;
  }
  .sl-elapsed {
    color: color-mix(in oklab, var(--muted-foreground) 75%, transparent);
    font-size: 0.6875rem;
    animation: sl-in 400ms ease-out both;
  }

  .sl-motif {
    width: 3.25rem;
    height: 1.25rem;
    flex: none;
    overflow: visible;
  }
  .sl-motif.sl-small {
    width: 2.5rem;
    height: 1rem;
  }
  .sl-edges path {
    fill: none;
    stroke: color-mix(in oklab, var(--muted-foreground) 40%, transparent);
    stroke-width: 1;
  }
  .sl-pulses path {
    fill: none;
    stroke: var(--primary);
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-dasharray: 14 86;
    stroke-dashoffset: 14;
    animation: sl-travel 2.2s linear infinite;
  }
  .sl-deep .sl-pulses path {
    animation-duration: 1.3s;
    stroke-width: 2.2;
  }
  .sl-nodes circle {
    fill: var(--muted-foreground);
    animation: sl-node 2s ease-in-out infinite;
    transform-box: fill-box;
    transform-origin: center;
  }
  .sl-deep .sl-nodes circle {
    fill: var(--primary);
    animation-duration: 1.4s;
  }

  @keyframes sl-in {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }
  @keyframes sl-phase {
    from {
      opacity: 0;
      transform: translateY(3px);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }
  @keyframes sl-sweep {
    0% {
      transform: translateX(-100%);
    }
    60%,
    100% {
      transform: translateX(100%);
    }
  }
  @keyframes sl-travel {
    0% {
      stroke-dashoffset: 14;
      opacity: 0;
    }
    10% {
      opacity: 1;
    }
    55% {
      stroke-dashoffset: -86;
      opacity: 1;
    }
    70%,
    100% {
      stroke-dashoffset: -86;
      opacity: 0;
    }
  }
  @keyframes sl-node {
    0%,
    100% {
      opacity: 0.45;
      transform: scale(0.85);
    }
    50% {
      opacity: 1;
      transform: scale(1.2);
    }
  }
  @keyframes sl-grow {
    0%,
    100% {
      transform: scaleY(0.82);
    }
    50% {
      transform: scaleY(1);
    }
  }

  /* Reduced motion (OS setting; the app has no setting of its own): a still skeleton and a
     still motif, no sweep, the phase text does not change either (see the script). */
  @media (prefers-reduced-motion: reduce) {
    .sl-root,
    .sl-root::before,
    .sl-root::after,
    .sl-col,
    .sl-pulses path,
    .sl-nodes circle,
    .sl-phase,
    .sl-elapsed {
      animation: none !important;
    }
    .sl-root::before,
    .sl-root::after {
      display: none;
    }
    .sl-pulses path {
      stroke-dasharray: none;
      opacity: 0.5;
    }
    .sl-nodes circle {
      opacity: 0.8;
    }
  }
</style>
