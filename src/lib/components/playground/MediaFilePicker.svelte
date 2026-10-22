<script lang="ts">
  import { t } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Upload, X } from '@lucide/svelte';
  let { id, accept, name, busy = false, onpick, onremove }: { id: string; accept: string; name: string; busy?: boolean; onpick: (file: File) => void; onremove: () => void } = $props();
  let input: HTMLInputElement;
</script>
<div role="group" aria-label={t('gen.choose_file')} class="flex flex-wrap items-center gap-3 rounded-md border border-dashed border-input bg-background p-4"
  ondragover={e => e.preventDefault()} ondrop={e => { e.preventDefault(); const file = e.dataTransfer?.files[0]; if (!busy && file) onpick(file); }}>
  <input {id} bind:this={input} type="file" {accept} class="sr-only" tabindex="-1" aria-label={t('gen.choose_file')} disabled={busy} onchange={e => { const file = e.currentTarget.files?.[0]; e.currentTarget.value = ''; if (file) onpick(file); }} />
  <Button variant="outline" disabled={busy} onclick={() => input.click()}><Upload aria-hidden="true" />{t('gen.choose_file')}</Button>
  <span class="min-w-0 flex-1 break-all text-sm text-muted-foreground" role="status">{name || t('gen.drop_file')}</span>
  {#if name}<Button variant="ghost" disabled={busy} onclick={onremove}><X aria-hidden="true" />{t('gen.remove_file')}</Button>{/if}
</div>
