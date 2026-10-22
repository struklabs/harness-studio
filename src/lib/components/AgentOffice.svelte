<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { Bot, Cpu, ListTodo } from '@lucide/svelte';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import OfficeFloor from '#lib/office/OfficeFloor.svelte';
  import OfficeIso from '#lib/office/OfficeIso.svelte';
  import OfficeWizard from '#lib/office/OfficeWizard.svelte';
  import { dimensions, presets, newScene, roomKey, validateScene, statusColor, eventStatus, type Scene, type Character, type Room, type Furniture } from '#lib/office/scene.js';
  import { OfficeEditor } from '#lib/office/editor.js';
  import { officeApi, officeEvents, type RunDetail } from '#lib/office/api.js';
  import { onWindowActivity, windowActive, holdReleaseWhile } from '#lib/windowActivity.js';
  import { officeLive } from '#lib/stores/officeLive.svelte.js';
  import { projects } from '#lib/stores/projects.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { isDesktop } from '#lib/transport.js';
  /** A copy of a preset; built-in names and their rooms read in the UI language. */
  function fromPreset(preset:Scene){
    const builtin=preset.id.startsWith('builtin-');const next=newScene(preset,builtin?t(`office.preset_${preset.id.slice(8)}` as TKey):preset.name);
    if(builtin)for(const room of next.rooms){const key=roomKey(room.name);const name=t(key as TKey);if(name!==key)room.name=name;}
    return next;
  }
  /** A running job holds the GPU; "running tool" is the agent wording. */
  const statusLabel=(c:Character)=>t(c.kind==='job'&&c.status==='running_tool'?'office.status_job_running':`office.status_${c.status}` as TKey);
  let editor=$state.raw(new OfficeEditor(fromPreset(presets[0])));
  let scene=$state.raw(untrack(()=>editor.scene)),baseline=$state(''),offices=$state.raw<Scene[]>([]),templates=$state.raw<Scene[]>(presets);
  let characters=$state<Character[]>([]),active=$state(true),connected=$state(false),error=$state(''),notice=$state(''),busy=$state(false),loaded=$state(false);
  let editing=$state(false),wizard=$state(false),selected=$state(''),selectedPreset=$state('builtin-studio'),view=$state<'floor'|'iso'>('floor');
  let chosen=$state<Character|null>(null),detail=$state<RunDetail|null>(null),detailLoading=$state(false),detailError=$state('');
  let pendingDelete=$state<{id:string;presets:boolean}|null>(null),pendingLoad=$state.raw<Scene|null>(null);
  let presetName=$state('');let fileInput:HTMLInputElement;
  let dirty=$derived(JSON.stringify(scene)!==baseline);
  // A notice about the last action (import, preset) is stale once the scene changes again.
  let noticeScene:Scene|null=null;
  function say(text:string){notice=text;noticeScene=scene;}
  $effect(()=>{const current=scene;untrack(()=>{if(notice&&current!==noticeScene)notice='';});});
  let fixture=$state<Character[]>([]);
  let actors=$derived([...characters.filter(c=>c.id!==officeLive.character?.id),...(officeLive.character?[officeLive.character]:[]),...fixture]);
  let item=$derived([...scene.rooms,...scene.furniture].find(v=>v.id===selected));
  let selectedRoom=$derived(scene.rooms.find(v=>v.id===selected));
  let selectedDesk=$derived(scene.furniture.find(v=>v.id===selected && v.kind==='desk'));
  let focusedCharacter=$derived(chosen?actors.find(c=>c.id===chosen?.id)??chosen:null);
  const furnitureKinds=['desk','meeting','kitchen','plant','board'] as const;
  function fail(e:unknown){error=e instanceof Error && e.message.startsWith('office.')?t(e.message as TKey):t('office.request_failed');}
  function change(fn:(s:Scene)=>void){try{scene=editor.transact(fn);error='';notice='';}catch(e){fail(e);}}
  function adopt(next:Scene,saved=false){editor=new OfficeEditor(validateScene(next));scene=editor.scene;baseline=saved?JSON.stringify(scene):'';selected='';editing=false;error='';}
  function requestLoad(next:Scene){if(dirty)pendingLoad=next;else adopt(next,true);}
  async function documents(){try{const [a,b]=await Promise.all([officeApi.list(),officeApi.list(true)]);offices=a;templates=b;return a;}catch(e){fail(e);return [];}}
  let refreshing=false,refreshAgain=false,activitySnapshot='';
  async function refresh(){
    if(!active)return;
    if(refreshing){refreshAgain=true;return;}
    refreshing=true;
    try{do{refreshAgain=false;const next=await officeApi.activity();if(!active)return;const snapshot=JSON.stringify(next);if(snapshot!==activitySnapshot){characters=next;activitySnapshot=snapshot;}connected=true;if(chosen){const current=next.find(c=>c.id===chosen?.id)??chosen;chosen=current;await loadDetail(current);}}while(refreshAgain&&active);}
    catch(e){connected=false;fail(e);}finally{refreshing=false;}
  }
  onMount(()=>{
    let disposed=false,stop:undefined|(()=>void);active=windowActive();
    void documents().then(a=>{if(disposed)return;if(a[0])adopt(a[0],true);loaded=true;});
    void refresh();if(isDesktop())void projects.load();
    void officeEvents(()=>void refresh(),()=>void documents(),ok=>connected=ok).then(fn=>{if(disposed)fn();else stop=fn;}).catch(fail);
    const off=onWindowActivity(v=>{active=v;if(v)void refresh();});
    // Snapshot refresh also covers state owned by services without an event producer.
    const timer=setInterval(()=>{if(active)void refresh();},5000);
    const release=holdReleaseWhile('agent-office',()=>dirty);
    // Dev only: `?office_fixture=30` (or localStorage `hs_office_fixture`) adds synthetic characters whose status changes every second.
    let fixtureTimer:ReturnType<typeof setInterval>|undefined;
    if(import.meta.env.DEV)void import('#lib/dev/officeFixtures.js').then(m=>{const count=m.officeFixtureCount(location.search,localStorage.getItem('hs_office_fixture'));if(!count||disposed)return;let tick=0;fixture=m.fixtureCharacters(count,tick);fixtureTimer=setInterval(()=>{if(active)fixture=m.fixtureCharacters(count,++tick);},1000);});
    return ()=>{disposed=true;active=false;stop?.();off();release();clearInterval(timer);clearInterval(fixtureTimer);};
  });
  async function save(){busy=true;try{const next=await officeApi.save(scene);baseline=JSON.stringify(next);await documents();notice='';error='';}catch(e){fail(e);}finally{busy=false;}}
  async function savePreset(){busy=true;try{const template=newScene(scene,presetName.trim()||scene.name);await officeApi.save(template,true);selectedPreset=template.id;await documents();say(t('office.preset_saved'));}catch(e){fail(e);}finally{busy=false;}}
  async function renamePreset(){const old=templates.find(v=>v.id===selectedPreset);if(!old||old.id.startsWith('builtin-'))return;busy=true;try{await officeApi.save({...old,name:presetName.trim()},true);await documents();say(t('office.preset_saved'));}catch(e){fail(e);}finally{busy=false;}}
  async function remove(){if(!pendingDelete)return;busy=true;try{const target=pendingDelete;await officeApi.remove(target.id,target.presets);await documents();if(!target.presets&&target.id===scene.id){const next=offices.find(v=>v.id!==target.id);adopt(next??fromPreset(presets[0]),!!next);}if(target.presets)selectedPreset='builtin-studio';pendingDelete=null;}catch(e){fail(e);}finally{busy=false;}}
  function duplicate(){adopt(newScene(scene,`${scene.name} ${t('office.copy_suffix')}`));}
  async function importScene(e:Event){const input=e.currentTarget as HTMLInputElement;const file=input.files?.[0];if(!file)return;try{if(file.size>1024*1024)throw new Error('office.invalid_scene');const parsed=validateScene(JSON.parse(await file.text()));const next=newScene(parsed);if(dirty)pendingLoad=next;else adopt(next);say(t('office.imported'));}catch{fail(new Error('office.invalid_scene'));}finally{input.value='';}}
  function exportScene(){const blob=new Blob([JSON.stringify(scene,null,2)+'\n'],{type:'application/json'}),url=URL.createObjectURL(blob),a=document.createElement('a');a.href=url;a.download=`office-${scene.id}.json`;a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);}
  function move(id:string,x:number,y:number){const target=[...scene.rooms,...scene.furniture].find(v=>v.id===id);if(target)try{scene=editor.move(id,{x,y,width:target.width,height:target.height});error='';}catch(e){fail(e);}}
  function bounds(key:'x'|'y'|'width'|'height',value:number){if(item)try{scene=editor.move(item.id,{x:item.x,y:item.y,width:item.width,height:item.height,[key]:value});error='';}catch(e){fail(e);}}
  function assign(desk:string,agent:string){try{scene=editor.assign(desk,agent);error='';}catch(e){fail(e);}}
  function add(kind:'room'|Furniture['kind']){
    const [w,h]=dimensions[scene.size];
    if(kind==='room'){
      for(let y=0;y<=h-3;y++)for(let x=0;x<=w-4;x++)if(!scene.rooms.some(r=>x<r.x+r.width&&r.x<x+4&&y<r.y+r.height&&r.y<y+3)){
        const room:Room={id:crypto.randomUUID(),name:t('office.department_default'),color:'#88b9b0',x,y,width:4,height:3};change(s=>s.rooms.push(room));selected=room.id;return;
      }fail(new Error('office.no_room_space'));return;
    }
    const id=crypto.randomUUID();change(s=>s.furniture.push({id,kind,x:1,y:1,width:kind==='desk'?2:1,height:1}));selected=id;
  }
  async function open(c:Character){chosen=c;detail=null;detailError='';detailLoading=false;await loadDetail(c,true);}
  async function loadDetail(c:Character,initial=false){
    if(c.id==='playground'||c.kind!=='agent'||!c.run_id)return;
    if(initial)detailLoading=true;
    const id=c.id,run=c.run_id;
    try{const value=await officeApi.run(run);if(active&&chosen?.id===id){detail=value;detailError='';if(value&&!characters.some(actor=>actor.id===id))chosen={...c,status:eventStatus(value.status)};}}
    catch{if(active&&chosen?.id===id)detailError=t('office.detail_failed');}
    finally{if(chosen?.id===id)detailLoading=false;}
  }
  function keydown(e:KeyboardEvent){if(!editing || e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLSelectElement)return;if((e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==='z'){e.preventDefault();scene=e.shiftKey?editor.redo():editor.undo();}else if(item&&['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(e.key)){e.preventDefault();move(item.id,item.x+(e.key==='ArrowLeft'?-1:e.key==='ArrowRight'?1:0),item.y+(e.key==='ArrowUp'?-1:e.key==='ArrowDown'?1:0));}}
</script>
<svelte:window onkeydown={keydown}/>
<div class="office-page">
  <header><div><p class="eyebrow">{t('office.agents_group')}</p><h1>{t('office.title')}</h1><p class="subtitle">{t('office.subtitle')}</p></div><span class="connection" class:online={connected} role="status">{t(connected?'office.live':'office.reconnecting')}</span></header>
  {#if error}<p class="message error" role="alert">{error}</p>{/if}{#if notice}<p class="message" role="status">{notice}</p>{/if}
  {#if pendingDelete}<div class="confirm" role="alert"><p>{t('office.delete_confirm')}</p><button class="danger" disabled={busy} onclick={remove}>{t('office.delete')}</button><button onclick={()=>pendingDelete=null}>{t('office.cancel')}</button></div>{/if}
  {#if pendingLoad}<div class="confirm" role="alert"><p>{t('office.discard_confirm')}</p><button onclick={()=>{if(pendingLoad)adopt(pendingLoad,offices.some(v=>v.id===pendingLoad?.id));pendingLoad=null;}}>{t('office.discard')}</button><button onclick={()=>pendingLoad=null}>{t('office.cancel')}</button></div>{/if}
  <div class="toolbar">
    <label class="sr-only" for="office-select">{t('office.offices')}</label><select id="office-select" value={offices.some(v=>v.id===scene.id)?scene.id:''} onchange={e=>{const next=offices.find(v=>v.id===e.currentTarget.value);if(next)requestLoad(next);}}><option value="">{t('office.unsaved_office')}</option>{#each offices as o}<option value={o.id}>{o.name}</option>{/each}</select>
    <button disabled={busy||!loaded} onclick={()=>wizard=true}>{t('office.new')}</button><button disabled={busy} onclick={duplicate}>{t('office.duplicate')}</button><button class="primary" disabled={busy||!loaded||!dirty} onclick={save}>{t('office.save')}</button>
    <button disabled={!offices.some(v=>v.id===scene.id)||busy} onclick={()=>pendingDelete={id:scene.id,presets:false}}>{t('office.delete')}</button><span class="spacer"></span><button onclick={exportScene}>{t('office.export')}</button><button onclick={()=>fileInput.click()}>{t('office.import')}</button><input bind:this={fileInput} type="file" accept="application/json,.json" onchange={importScene} class="sr-only" aria-label={t('office.import')}/>
  </div>
  {#if wizard}<OfficeWizard characters={actors.filter(c=>c.kind==='agent')} projects={projects.list} oncancel={()=>wizard=false} oncomplete={s=>{adopt(s);wizard=false;}}/>
  {:else}
    <div class="scene-head"><label>{t('office.name')}<input value={scene.name} maxlength="200" onchange={e=>change(s=>s.name=e.currentTarget.value)}/></label><span class="save-state">{t(dirty?'office.unsaved':'office.saved')}</span><div class="view-switch" aria-label={t('office.view')}><button aria-pressed={view==='floor'} onclick={()=>view='floor'}>{t('office.floor')}</button><button aria-pressed={view==='iso'} onclick={()=>{view='iso';editing=false;}}>{t('office.isometric')}</button><button aria-pressed={editing} disabled={view==='iso'} onclick={()=>editing=!editing}>{t('office.editor')}</button></div></div>
    <div class="workspace" class:with-editor={editing}>
      <div class="scene-area">{#if view==='iso'}<OfficeIso {scene} characters={actors} {active} onopen={open}/>{:else}<OfficeFloor {scene} characters={actors} {active} {editing} {selected} onselect={id=>selected=id} onmove={move} onassign={assign} onopen={open}/>{/if}
      <div class="legend">{#each ['idle','thinking','running_tool','waiting_for_approval','error','sleeping'] as status}<span><i style:background={statusColor[status as keyof typeof statusColor]}></i>{t(`office.status_${status}` as TKey)}</span>{/each}</div>
      {#if editing}<p class="hint">{t('office.editor_hint')}</p>{/if}
      </div>
      {#if editing}<aside class="editor" aria-label={t('office.editor')}>
        <h2>{t('office.editor')}</h2><div class="buttons"><button disabled={!(scene&&editor.canUndo)} onclick={()=>scene=editor.undo()}>{t('office.undo')}</button><button disabled={!(scene&&editor.canRedo)} onclick={()=>scene=editor.redo()}>{t('office.redo')}</button></div>
        <label>{t('office.theme')}<select value={scene.theme} onchange={e=>change(s=>s.theme=e.currentTarget.value as Scene['theme'])}>{#each ['classic','pixel','night'] as th}<option value={th}>{t(`office.theme_${th}` as TKey)}</option>{/each}</select></label>
        <div class="buttons"><button onclick={()=>add('room')}>{t('office.add_room')}</button>{#each furnitureKinds as kind}<button onclick={()=>add(kind)}>{t(`office.furniture_${kind}`)}</button>{/each}</div>
        <label>{t('office.selection')}<select bind:value={selected}><option value="">{t('office.select_item')}</option>{#each scene.rooms as r}<option value={r.id}>{r.name}</option>{/each}{#each scene.furniture as f}<option value={f.id}>{t(`office.furniture_${f.kind}`)} ({f.x}, {f.y})</option>{/each}</select></label>
        {#if item}
          {#if selectedRoom}<label>{t('office.room_name')}<input value={selectedRoom.name} onchange={e=>change(s=>{const r=s.rooms.find(v=>v.id===selected);if(r)r.name=e.currentTarget.value;})}/></label><label>{t('office.color')}<input type="color" value={selectedRoom.color} onchange={e=>change(s=>{const r=s.rooms.find(v=>v.id===selected);if(r)r.color=e.currentTarget.value;})}/></label>{/if}
          <div class="bounds">{#each ['x','y','width','height'] as key}<label>{t(`office.${key}` as TKey)}<input type="number" min={key==='width'||key==='height'?1:0} max="32" value={item[key as keyof typeof item]} onchange={e=>{const el=e.currentTarget;const k=key as 'x'|'y'|'width'|'height';bounds(k,Number(el.value));if(item)el.value=String(item[k]);}}/></label>{/each}</div>
          {#if selectedRoom}<label>{t('office.project')}<select value={scene.room_projects[selected]??''} onchange={e=>change(s=>{if(e.currentTarget.value)s.room_projects[selected]=e.currentTarget.value;else delete s.room_projects[selected];})}><option value="">{t('office.unassigned')}</option>{#each projects.list as project}<option value={project.id}>{project.name}</option>{/each}</select></label>{/if}
          {#if selectedDesk}<label>{t('office.agent')}<select value={scene.desk_agents[selected]??''} onchange={e=>assign(selected,e.currentTarget.value)}><option value="">{t('office.unassigned')}</option>{#each actors as c}<option value={c.id}>{c.name}</option>{/each}</select></label>{/if}
          <button class="danger" disabled={!!selectedRoom&&scene.rooms.length===1} onclick={()=>{try{scene=editor.remove(selected);selected='';}catch(e){fail(e);}}}>{t('office.remove_item')}</button>
        {/if}
      </aside>{/if}
    </div>
    <details class="presets"><summary>{t('office.presets')}</summary><div class="preset-tools"><label>{t('office.preset')}<select bind:value={selectedPreset}>{#each templates as template}<option value={template.id}>{template.id.startsWith('builtin-')?t(`office.preset_${template.id.slice(8)}` as TKey):template.name}</option>{/each}</select></label><button onclick={()=>{const preset=templates.find(v=>v.id===selectedPreset);if(preset){const next=fromPreset(preset);if(dirty)pendingLoad=next;else adopt(next);}}}>{t('office.use_preset')}</button><label>{t('office.preset_name')}<input bind:value={presetName}/></label><button disabled={busy} onclick={savePreset}>{t('office.save_preset')}</button><button disabled={busy||selectedPreset.startsWith('builtin-')||!presetName.trim()} onclick={renamePreset}>{t('office.rename')}</button><button disabled={busy||selectedPreset.startsWith('builtin-')} onclick={()=>pendingDelete={id:selectedPreset,presets:true}}>{t('office.delete')}</button></div></details>
    <section class="activity" aria-labelledby="office-activity-title"><div class="activity-header"><h2 id="office-activity-title">{t('office.activity')}</h2><span>{actors.length}</span><button onclick={()=>void refresh()}>{t('office.refresh')}</button></div><p class="hint">{t('office.activity_hint')}</p>
      <ul class="character-list">{#each actors as c (c.id)}<li><button class:chosen={chosen?.id===c.id} onclick={()=>open(c)} draggable={editing} ondragstart={e=>e.dataTransfer?.setData('text/plain',c.id)}><span class="character-icon" style:background={statusColor[c.status]}>{#if c.kind==='device'}<Cpu class="size-3.5" />{:else if c.kind==='job'}<ListTodo class="size-3.5" />{:else}<Bot class="size-3.5" />{/if}</span><span class="character-name">{c.name}<small>{t(`office.kind_${c.kind}` as TKey)}{#if c.agent} · {c.agent}{/if}</small></span><span class="status-badge" style:--status={statusColor[c.status]}>{statusLabel(c)}</span></button></li>{/each}</ul>
    </section>
    {#if focusedCharacter}<section class="detail" aria-labelledby="office-detail-title"><header><h2 id="office-detail-title">{focusedCharacter.name}</h2><button onclick={()=>chosen=null}>{t('office.close')}</button></header><p>{statusLabel(focusedCharacter)}</p>
      {#if focusedCharacter.id==='playground'}<p>{t(focusedCharacter.status==='waiting_for_approval'?'office.approval_hint':'office.chat_hint')}</p><button class="primary" onclick={()=>officeLive.open?.()}>{t(focusedCharacter.status==='waiting_for_approval'?'office.open_approval':'office.open_run')}</button>
      {:else if detailLoading}<p role="status">{t('office.loading')}</p>{:else if detailError}<p role="alert">{detailError}</p>{:else if detail}{#if focusedCharacter.id.startsWith('taskflow-')&&focusedCharacter.status==='waiting_for_approval'}<p>{t('office.taskrun_approval_hint')}</p>{/if}<h3>{t('office.task')}</h3><pre>{detail.prompt}</pre><h3>{t('office.output')}</h3><pre>{detail.output||t('office.no_output')}</pre>{#if detail.error}<p role="alert">{detail.error}</p>{/if}<p>{detail.audit.join(' → ')}</p>
      {:else}<p>{t(focusedCharacter.kind==='device'?'office.device_hint':focusedCharacter.kind==='job'?'office.job_hint':'office.no_run')}</p>{#if focusedCharacter.kind==='job'}<button onclick={()=>app.requestedTab='dashboard'}>{t('office.open_jobs')}</button>{/if}{/if}
    </section>{/if}
  {/if}
</div>
<style>
 .office-page{padding:1.5rem;max-width:1600px;margin:auto;width:100%;height:100%;overflow:auto;color:var(--foreground)}header{display:flex;align-items:center;justify-content:space-between;gap:1rem}h1{font-size:1.65rem;letter-spacing:-.035em;font-weight:650}h2{font-weight:650;font-size:1rem}h3{font-weight:600;font-size:.9rem;margin-top:1rem}.eyebrow{color:var(--muted-foreground);font-size:.7rem;font-weight:650;text-transform:uppercase;letter-spacing:.14em}.subtitle,.hint{color:var(--muted-foreground);font-size:.85rem;margin:.5rem 0}.connection{font-size:.75rem;color:var(--muted-foreground);white-space:nowrap}.connection:before{content:'';display:inline-block;width:7px;height:7px;background:#9c8a7d;border-radius:50%;margin-right:6px}.connection.online:before{background:#609c7b}.toolbar{display:flex;align-items:center;flex-wrap:wrap;gap:.45rem;padding:1rem 0;border-bottom:1px solid var(--border);margin-bottom:1rem}button,select,input{border:1px solid var(--border);border-radius:6px;background:var(--background);font-size:.8rem;padding:.45rem .65rem;color:var(--foreground)}button{cursor:pointer;white-space:nowrap}button:hover{background:var(--accent)}button:focus-visible,input:focus-visible,select:focus-visible,summary:focus-visible{outline:2px solid var(--ring);outline-offset:2px}button:disabled{opacity:.45;cursor:default}.primary{background:var(--primary);color:var(--primary-foreground)}.danger{color:var(--destructive)}button[aria-pressed=true]{background:var(--accent);border-color:var(--ring)}.spacer{flex:1}.scene-head{display:flex;align-items:end;gap:1rem;margin-bottom:1rem;flex-wrap:wrap}label{font-size:.75rem;color:var(--muted-foreground);display:grid;gap:.3rem}label input,label select{color:var(--foreground)}.scene-head input{font-size:1rem;font-weight:600;min-width:240px}.save-state{font-size:.7rem;color:var(--muted-foreground);align-self:center}.view-switch{display:flex;gap:.4rem;margin-left:auto}.workspace{display:grid;gap:1rem;align-items:start}.workspace.with-editor{grid-template-columns:minmax(0,1fr) 260px}.scene-area{min-width:0}.legend{display:flex;gap:1rem;flex-wrap:wrap;font-size:.7rem;color:var(--muted-foreground);padding:.8rem .3rem}.legend span{display:flex;gap:.35rem;align-items:center}.legend i{width:7px;height:7px;border-radius:50%}.editor{display:grid;gap:.75rem;padding:1rem;background:var(--card);border:1px solid var(--border);border-radius:10px}.buttons{display:flex;gap:.3rem;flex-wrap:wrap}.bounds{display:grid;grid-template-columns:1fr 1fr;gap:.5rem}.bounds input{width:100%}.presets{padding:.8rem 0;border-block:1px solid var(--border);margin-top:.5rem;font-size:.85rem}summary{cursor:pointer}.preset-tools{display:flex;gap:.6rem;flex-wrap:wrap;align-items:end;padding-top:.8rem}.activity{margin-top:1.25rem}.activity-header{display:flex;align-items:center;gap:.6rem}.activity-header>span{font-size:.75rem;color:var(--muted-foreground)}.activity-header>button{margin-left:auto}.character-list{display:grid;grid-template-columns:repeat(auto-fill,minmax(280px,1fr));gap:.5rem;margin-top:.75rem}.character-list button{display:flex;align-items:center;gap:.6rem;width:100%;text-align:left;padding:.7rem;background:var(--card)}.character-list .chosen{border-color:var(--ring)}.character-name{min-width:0;flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.character-name small{display:block;font-size:.65rem;font-weight:400;color:var(--muted-foreground);margin-top:.15rem}.character-icon{display:inline-flex;align-items:center;border-radius:8px;color:white;padding:.3rem .45rem}.status-badge{font-size:.65rem;border:1px solid color-mix(in srgb,var(--status) 45%,transparent);background:color-mix(in srgb,var(--status) 10%,transparent);padding:.2rem .4rem;border-radius:99px;white-space:normal}.detail{margin-top:1rem;padding:1rem;border:1px solid var(--border);border-radius:10px;background:var(--card);font-size:.85rem}.detail p{margin:.6rem 0;color:var(--muted-foreground)}pre{white-space:pre-wrap;overflow-wrap:anywhere;max-height:300px;overflow:auto;padding:.7rem;background:var(--muted);border-radius:6px;font-size:.8rem}.message,.confirm{padding:.75rem;border:1px solid var(--border);border-radius:8px;margin:.7rem 0;font-size:.85rem}.error{color:var(--destructive)}.confirm{display:flex;gap:.5rem;align-items:center;flex-wrap:wrap}.confirm p{flex:1}@media(max-width:1000px){.workspace.with-editor{grid-template-columns:1fr}.editor{grid-template-columns:1fr 1fr}}@media(max-width:650px){.office-page{padding:.8rem}.scene-head{align-items:start}.view-switch{margin:0}.editor{grid-template-columns:1fr}}
</style>
