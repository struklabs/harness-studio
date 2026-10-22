<script lang="ts">
  import GenerationField from './GenerationField.svelte';
  // Draws an inpainting mask over the start image: white strokes are redrawn, the rest is kept.
  // The mask leaves as a PNG of the start image's own pixel size (black and white), which is
  // what the gateway wants (`mask` of /v1/images/edits).
  import { t } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Check, Eraser, Trash2, X } from '@lucide/svelte';
  import { dataUrl } from './image';
  import type { ImageResult } from './image';

  let { image, onapply, oncancel }: { image: ImageResult; onapply: (mask: ImageResult) => void; oncancel: () => void } = $props();

  let canvas = $state<HTMLCanvasElement | null>(null);
  let natural = $state<{ w: number; h: number } | null>(null);
  let brush = $state(10);
  let erasing = $state(false);
  let drawing = false;
  let last: { x: number; y: number } | null = null;
  let touched = $state(false);

  function loaded(e: Event) {
    const img = e.currentTarget as HTMLImageElement;
    natural = { w: img.naturalWidth, h: img.naturalHeight };
  }

  /** Brush size in start-image pixels: the slider is a percentage of the shorter side. */
  function radius(): number {
    if (!natural) return 8;
    return Math.max(2, (Math.min(natural.w, natural.h) * brush) / 200);
  }

  function point(e: PointerEvent): { x: number; y: number } {
    const r = canvas!.getBoundingClientRect();
    return { x: ((e.clientX - r.left) / r.width) * canvas!.width, y: ((e.clientY - r.top) / r.height) * canvas!.height };
  }

  function stroke(from: { x: number; y: number }, to: { x: number; y: number }) {
    const ctx = canvas!.getContext('2d')!;
    ctx.globalCompositeOperation = erasing ? 'destination-out' : 'source-over';
    ctx.strokeStyle = '#f43f5e';
    ctx.fillStyle = '#f43f5e';
    ctx.lineWidth = radius() * 2;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.beginPath();
    ctx.moveTo(from.x, from.y);
    ctx.lineTo(to.x, to.y);
    ctx.stroke();
    ctx.beginPath();
    ctx.arc(to.x, to.y, radius(), 0, Math.PI * 2);
    ctx.fill();
    touched = true;
  }

  function down(e: PointerEvent) {
    if (!canvas || !natural) return;
    canvas.setPointerCapture(e.pointerId);
    drawing = true;
    last = point(e);
    stroke(last, last);
  }

  function move(e: PointerEvent) {
    if (!drawing || !last) return;
    const p = point(e);
    stroke(last, p);
    last = p;
  }

  function up() {
    drawing = false;
    last = null;
  }

  function clear() {
    if (!canvas) return;
    canvas.getContext('2d')!.clearRect(0, 0, canvas.width, canvas.height);
    touched = false;
  }

  /** The strokes (drawn in a tint) on black, as a black and white PNG: white = redraw. */
  function apply() {
    if (!canvas || !natural) return;
    const out = document.createElement('canvas');
    out.width = natural.w;
    out.height = natural.h;
    const ctx = out.getContext('2d')!;
    ctx.fillStyle = '#000';
    ctx.fillRect(0, 0, out.width, out.height);
    ctx.drawImage(canvas, 0, 0);
    // Anything the brush only half covered (soft edges) is made fully white or black.
    const data = ctx.getImageData(0, 0, out.width, out.height);
    for (let i = 0; i < data.data.length; i += 4) {
      const v = data.data[i] > 127 ? 255 : 0;
      data.data[i] = data.data[i + 1] = data.data[i + 2] = v;
      data.data[i + 3] = 255;
    }
    ctx.putImageData(data, 0, 0);
    const url = out.toDataURL('image/png');
    onapply({ data: url.slice(url.indexOf(',') + 1), format: 'png' });
  }
</script>

<div class="flex flex-col gap-2 rounded-md border border-border bg-muted/20 p-3" role="group" aria-label={t('img.mask_draw_title')}>
  <p class="text-xs text-muted-foreground">{t('img.mask_draw_hint')}</p>
  <div class="relative w-full max-w-[480px] self-start overflow-hidden rounded-md border border-border bg-muted">
    <img src={dataUrl(image)} alt={t('img.init_label')} class="block h-auto w-full select-none" draggable="false" onload={loaded} />
    {#if natural}
      <canvas
        bind:this={canvas}
        width={natural.w}
        height={natural.h}
        class="absolute inset-0 size-full cursor-crosshair touch-none opacity-60"
        aria-label={t('img.mask_canvas')}
        onpointerdown={down}
        onpointermove={move}
        onpointerup={up}
        onpointercancel={up}
      ></canvas>
    {/if}
  </div>
  <div class="flex flex-wrap items-center gap-3">
    <GenerationField id="mask-brush" label={t('img.mask_brush')} help="gen.brush">
      <input id="mask-brush" type="range" min="1" max="40" bind:value={brush} class="w-32" />
    </GenerationField>
    <Button variant={erasing ? 'secondary' : 'outline'} size="sm" aria-pressed={erasing} onclick={() => (erasing = !erasing)}>
      <Eraser />{t('img.mask_erase')}
    </Button>
    <Button variant="outline" size="sm" disabled={!touched} onclick={clear}><Trash2 />{t('img.mask_clear')}</Button>
    <span class="flex-1"></span>
    <Button variant="ghost" size="sm" onclick={oncancel}><X />{t('img.mask_cancel')}</Button>
    <Button size="sm" disabled={!touched || !natural} onclick={apply}><Check />{t('img.mask_apply')}</Button>
  </div>
</div>
