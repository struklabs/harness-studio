<script lang="ts">
  // The image workbench: the built-in generator or the ComfyUI bridge. Shared by the Image
  // tab and the Image studio so the two never drift apart.
  import { t } from '#lib/i18n.svelte.js';
  import * as ToggleGroup from '#lib/components/ui/toggle-group/index.js';
  import ImagePanel from './ImagePanel.svelte';
  import ComfyPanel from './ComfyPanel.svelte';
  import { imageSession } from './image.svelte';
</script>

<div class="mx-auto flex w-full max-w-[960px] px-4 pt-4">
  <ToggleGroup.Root
    type="single"
    variant="outline"
    size="sm"
    value={imageSession.source}
    onValueChange={(v) => {
      if (v) imageSession.source = v as 'builtin' | 'comfyui';
    }}
    aria-label={t('comfy.source_label')}
  >
    <ToggleGroup.Item value="builtin" class="h-7 px-2 text-xs">{t('comfy.source_builtin')}</ToggleGroup.Item>
    <ToggleGroup.Item value="comfyui" class="h-7 px-2 text-xs">{t('comfy.source_comfyui')}</ToggleGroup.Item>
  </ToggleGroup.Root>
</div>
{#if imageSession.source === 'comfyui'}
  <ComfyPanel />
{:else}
  <ImagePanel />
{/if}
