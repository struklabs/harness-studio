<script lang="ts">
  interface Props {
    size?: number;
    showText?: boolean;
    version?: string;
  }

  let { size = 22, showText = true, version = 'v0.2.0' }: Props = $props();
</script>

<div class="inline-flex items-center gap-2.5 text-foreground">
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden="true" class="shrink-0">
    <rect x="2" y="2" width="20" height="20" rx="5" fill="currentColor" fill-opacity="0.06" stroke="currentColor" stroke-opacity="0.35" stroke-width="1.5" />
    <path d="M7 8H17M7 12H17M7 16H12.5" stroke="currentColor" stroke-width="2" stroke-linecap="round" />
    <circle cx="16.5" cy="16" r="2.25" fill="var(--primary)" />
  </svg>

  {#if showText}
    <div class="flex items-baseline gap-1.5 whitespace-nowrap">
      <span class="text-sm font-semibold tracking-tight">Harness Studio</span>
      {#if version}
        <span class="font-num text-xs text-muted-foreground">{version}</span>
      {/if}
    </div>
  {/if}
</div>
