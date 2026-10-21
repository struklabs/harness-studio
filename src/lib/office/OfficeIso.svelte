<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '#lib/i18n.svelte.js';
  import type { Scene, Character } from './scene';
  import type { IsoRenderer } from './isometric';
  let {scene,characters,active,onopen}:{scene:Scene;characters:Character[];active:boolean;onopen:(c:Character)=>void}=$props();
  let host:HTMLDivElement; let renderer:IsoRenderer|undefined=$state();let failed=$state(false);
  onMount(()=>{let disposed=false;void import('./isometric').then(m=>{if(!disposed)renderer=m.mountIsometric(host,onopen);}).catch(()=>failed=true);return ()=>{disposed=true;renderer?.dispose();};});
  $effect(()=>{if(renderer)try{renderer.update(scene,characters,active);}catch{failed=true;renderer.dispose();}});
</script>
{#if failed}<p role="status">{t('office.webgl_error')}</p>{/if}
<div bind:this={host} class="iso" aria-hidden="true"></div>
<style>.iso{width:100%;height:540px;max-height:65vh;border-radius:14px;overflow:hidden}.iso :global(canvas){display:block;width:100%;height:100%}</style>
