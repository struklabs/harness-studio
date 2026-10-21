<script lang="ts">
  import { t } from '#lib/i18n.svelte.js';
  import { wizardScene, type Scene, type Character, type OfficeSize, type OfficeTheme } from './scene';
  let {characters,projects,oncomplete,oncancel}:{characters:Character[];projects:{id:string;name:string}[];oncomplete:(s:Scene)=>void;oncancel:()=>void}=$props();
  let step=$state(0),size=$state<OfficeSize>('small'),theme=$state<OfficeTheme>('classic'),name=$state(t('office.new_office')),departments=$state(t('office.department_default'));
  let agentRooms=$state<Record<string,string>>({}),roomProjects=$state<Record<string,string>>({}),error=$state('');
  let names=$derived(departments.split('\n').map(s=>s.trim()).filter(Boolean));
  const steps=['size','departments','assignments','theme','done'] as const;
  function build() {
    if(new Set(names).size!==names.length)throw new Error('office.duplicate_departments');
    const assignments=Object.fromEntries(names.map(n=>[n,{agents:characters.filter(c=>agentRooms[c.id]===n).map(c=>c.id),project:roomProjects[n]}]));
    return wizardScene(size,names,assignments,theme,name);
  }
  function next(){try{error='';if(step===1 && (!names.length||names.length>8||new Set(names).size!==names.length))throw new Error('office.invalid_scene');if(step===3)build();step++;}catch(e){error=e instanceof Error?e.message:'office.invalid_scene';}}
</script>
<section class="wizard" aria-labelledby="office-wizard-title">
  <h2 id="office-wizard-title">{t('office.wizard')}</h2>
  <ol class="steps" aria-label={t('office.wizard')} >{#each steps as s,i}<li aria-current={step===i?'step':undefined}>{i+1}. {t(`office.step_${s}`)}</li>{/each}</ol>
  {#if step===0}
    <label>{t('office.name')}<input bind:value={name} maxlength="200" /></label>
    <label>{t('office.size')}<select bind:value={size}>{#each ['small','medium','large'] as s}<option value={s}>{t(`office.size_${s}` as Parameters<typeof t>[0])}</option>{/each}</select></label>
  {:else if step===1}
    <label>{t('office.departments')}<textarea rows="5" bind:value={departments} aria-describedby="office-department-hint"></textarea></label>
    <p id="office-department-hint">{t('office.departments_hint')}</p>
  {:else if step===2}
    <p>{t('office.assignments_hint')}</p>
    {#each characters as c}<label>{c.name}<select value={agentRooms[c.id]??''} onchange={e=>agentRooms[c.id]=e.currentTarget.value}><option value="">{t('office.unassigned')}</option>{#each names as n}<option value={n}>{n}</option>{/each}</select></label>{/each}
    {#each names as n}<label>{n} ? {t('office.project')}<select value={roomProjects[n]??''} onchange={e=>roomProjects[n]=e.currentTarget.value}><option value="">{t('office.unassigned')}</option>{#each projects as p}<option value={p.id}>{p.name}</option>{/each}</select></label>{/each}
  {:else if step===3}
    <label>{t('office.theme')}<select bind:value={theme}>{#each ['classic','pixel','night'] as th}<option value={th}>{t(`office.theme_${th}` as Parameters<typeof t>[0])}</option>{/each}</select></label>
  {:else}
    <h3>{name}</h3><p>{t('office.wizard_review',{rooms:names.length,agents:Object.values(agentRooms).filter(Boolean).length})}</p><p>{t('office.wizard_save_hint')}</p>
  {/if}
  {#if error}<p role="alert">{t(error as Parameters<typeof t>[0])}</p>{/if}
  <footer><button onclick={oncancel}>{t('office.cancel')}</button>{#if step>0}<button onclick={()=>step--}>{t('office.back')}</button>{/if}{#if step<4}<button class="primary" onclick={next} disabled={!name.trim()}>{t('office.next')}</button>{:else}<button class="primary" onclick={()=>{try{oncomplete(build());}catch{error='office.invalid_scene';}}}>{t('office.create')}</button>{/if}</footer>
</section>
<style>.wizard{max-width:680px;margin:1rem auto;padding:1.5rem;border:1px solid var(--border);border-radius:12px;background:var(--card)}h2{font-size:1.3rem;font-weight:650}.steps{display:flex;flex-wrap:wrap;gap:1rem;padding:1rem 0;font-size:.8rem;color:var(--muted-foreground)}li[aria-current]{color:var(--primary);font-weight:650}label{display:grid;gap:.35rem;margin:.8rem 0;font-size:.9rem}input,select,textarea{background:var(--background);border:1px solid var(--border);border-radius:6px;padding:.55rem;color:var(--foreground)}p{font-size:.85rem;color:var(--muted-foreground);margin:.8rem 0}footer{display:flex;justify-content:flex-end;gap:.5rem;margin-top:1.5rem}button{border:1px solid var(--border);border-radius:6px;padding:.5rem .8rem;cursor:pointer}.primary{background:var(--primary);color:var(--primary-foreground)}button:disabled{opacity:.5;cursor:default}</style>
