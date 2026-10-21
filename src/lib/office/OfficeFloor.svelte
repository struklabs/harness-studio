<script lang="ts">
  import { t } from '#lib/i18n.svelte.js';
  import { dimensions, positions, statusColor, type Scene, type Character } from './scene';
  let { scene, characters, active=true, editing=false, selected='', onselect=(_id:string)=>{}, onmove=(_id:string,_x:number,_y:number)=>{}, onassign=(_desk:string,_agent:string)=>{}, onopen=(_c:Character)=>{} }: {scene:Scene;characters:Character[];active?:boolean;editing?:boolean;selected?:string;onselect?:(id:string)=>void;onmove?:(id:string,x:number,y:number)=>void;onassign?:(desk:string,agent:string)=>void;onopen?:(c:Character)=>void}=$props();
  let floor:HTMLDivElement;
  let drag:{id:string;x:number;y:number;startX:number;startY:number}|null=null;
  let moved=false;
  let grid=$derived(dimensions[scene.size]);
  let actors=$derived(positions(scene,characters));
  function begin(e:PointerEvent,id:string,x:number,y:number) { if(!editing || e.button!==0) return; onselect(id); moved=false; drag={id,x,y,startX:e.clientX,startY:e.clientY}; (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId); }
  function end(e:PointerEvent) { if(!drag || !floor) return;const rect=floor.getBoundingClientRect();const dx=(e.clientX-drag.startX)/rect.width*grid[0],dy=(e.clientY-drag.startY)/rect.height*grid[1];moved=Math.abs(dx)+Math.abs(dy)>0.4;if(moved)onmove(drag.id,Math.round(drag.x+dx),Math.round(drag.y+dy));drag=null; }
</script>
<div bind:this={floor} class:pixel={scene.theme==='pixel'} class:night={scene.theme==='night'} class:paused={!active} class="floor" style:aspect-ratio={`${grid[0]} / ${grid[1]}`}>
  <svg viewBox={`0 0 ${grid[0]*40} ${grid[1]*40}`} aria-hidden="true">
    <defs><pattern id="office-grid" width="40" height="40" patternUnits="userSpaceOnUse"><path d="M 40 0 L 0 0 0 40" fill="none" stroke="currentColor" stroke-opacity="0.06" /></pattern></defs>
    <rect width="100%" height="100%" fill="url(#office-grid)" />
    {#each scene.rooms as room (room.id)}
      <rect x={room.x*40} y={room.y*40} width={room.width*40} height={room.height*40} rx={scene.theme==='pixel'?0:8} fill={room.color} fill-opacity="0.18" stroke={room.color} stroke-width="3" />
      <text x={room.x*40+12} y={room.y*40+20} fill="currentColor" font-size="12" font-weight="600">{room.name}</text>
    {/each}
    {#each scene.furniture as f (f.id)}
      <g transform={`translate(${f.x*40},${f.y*40})`}>
        {#if f.kind==='plant'}
          <rect x="12" y="22" width="16" height="12" rx="2" fill="#a47a59"/><circle cx="20" cy="15" r="13" fill="#548b69"/><circle cx="15" cy="10" r="8" fill="#79ad88"/>
        {:else}
          <rect x="2" y="5" width={f.width*40-4} height={f.height*40-6} rx={scene.theme==='pixel'?0:4} fill={f.kind==='board'?'#e1e9df':f.kind==='kitchen'?'#91a8af':'#bca182'} stroke="#776b60" stroke-width="2" />
          {#if f.kind==='desk'}<rect x={f.width*20-13} y="9" width="26" height="15" rx="2" fill="#435867"/><path d={`M ${f.width*20-7} 29 h 14`} stroke="#60717b" stroke-width="3"/>{/if}
          {#if f.kind==='meeting'}<circle cx="-3" cy={f.height*20} r="8" fill="#8d9aa8"/><circle cx={f.width*40+3} cy={f.height*20} r="8" fill="#8d9aa8"/>{/if}
        {/if}
      </g>
    {/each}
  </svg>
  {#if editing}
    {#each [...scene.rooms,...scene.furniture] as item (item.id)}
      <button class="edit-target" class:selected={selected===item.id} style:left={`${item.x/grid[0]*100}%`} style:top={`${item.y/grid[1]*100}%`} style:width={`${item.width/grid[0]*100}%`} style:height={`${item.height/grid[1]*100}%`}
        aria-label={'name' in item?item.name:t(`office.furniture_${item.kind}`)} onpointerdown={e=>begin(e,item.id,item.x,item.y)} onpointerup={end} onpointercancel={()=>drag=null} onclick={()=>{if(!moved)onselect(item.id);}}
        ondragover={e=>{if('kind' in item && item.kind==='desk')e.preventDefault();}} ondrop={e=>{e.preventDefault();if('kind' in item && item.kind==='desk')onassign(item.id,e.dataTransfer?.getData('text/plain')??'');}}></button>
    {/each}
  {/if}
  {#each actors as c (c.id)}
    <button class="actor" class:device={c.kind==='device'} class:thinking={c.status==='thinking'||c.status==='running_tool'} style:left={`${c.x/grid[0]*100}%`} style:top={`${c.y/grid[1]*100}%`} style:--badge={statusColor[c.status]} aria-label={`${c.name}: ${t(`office.status_${c.status}`)}`} title={`${c.name}: ${t(`office.status_${c.status}`)}`} onclick={()=>onopen(c)} draggable={editing} ondragstart={e=>e.dataTransfer?.setData('text/plain',c.id)}>
      <svg viewBox="0 0 40 48" aria-hidden="true">
        {#if c.kind==='device'}<rect x="8" y="12" width="24" height="27" rx="4" fill="#7993a3"/><path d="M14 20h12m-12 6h12m-12 6h5" stroke="#d9e9ed" stroke-width="2"/>
        {:else}<ellipse cx="20" cy="43" rx="14" ry="4" fill="#182b3a" opacity=".15"/><path d="M10 37 Q10 22 20 22 Q30 22 30 37 Z" fill={c.kind==='job'?'#bd986b':'#6b91b0'}/><circle cx="20" cy="16" r="9" fill="#d9bda4"/><path d="M11 16Q9 3 22 6Q31 6 29 17L25 12L13 13Z" fill="#5d5960"/><circle cx="17" cy="17" r="1" fill="#35434d"/><circle cx="23" cy="17" r="1" fill="#35434d"/>{/if}
      </svg><span class="badge">{c.status==='waiting_for_approval'?'?':c.status==='error'?'!':c.status==='sleeping'?'z':c.status==='thinking'?'…':c.status==='running_tool'?'•':''}</span>
    </button>
  {/each}
</div>
<style>
 .floor{position:relative;width:100%;background:#f2f0e9;color:#354752;border:1px solid #889a9d55;border-radius:14px;overflow:hidden;isolation:isolate}.floor.night{background:#202d3a;color:#d3e4e4}.floor>svg{width:100%;height:100%;display:block}.pixel svg{shape-rendering:crispEdges;image-rendering:pixelated}.pixel{border-radius:0}.actor{position:absolute;transform:translate(-50%,-55%);width:clamp(22px,4%,44px);aspect-ratio:40/48;cursor:pointer;border:0;background:transparent;padding:0;z-index:3}.actor svg{width:100%;height:100%;filter:drop-shadow(0 3px 2px #24344222)}.actor:focus-visible{outline:3px solid #547cc6;border-radius:6px}.badge{position:absolute;right:-2px;top:0;border:2px solid #fff;border-radius:99px;background:var(--badge);color:white;min-width:15px;height:17px;line-height:12px;font:bold 13px monospace;text-align:center}.edit-target{position:absolute;background:transparent;border:1px dashed transparent;z-index:1;touch-action:none;cursor:move}.edit-target:hover,.edit-target:focus-visible{border-color:#4e7db6;background:#4e7db611}.edit-target.selected{border:2px dashed #4e7db6;z-index:2}.thinking svg{animation:work 1.6s ease-in-out infinite}.paused *{animation-play-state:paused!important}@keyframes work{50%{transform:translateY(-2px)}}@media(prefers-reduced-motion:reduce){.thinking svg{animation:none}}
</style>
