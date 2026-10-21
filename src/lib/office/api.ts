import { invoke } from '#lib/ipc.js';
import { isDesktop, control, CONTROL_BASE } from '#lib/transport.js';
import { listen } from '@tauri-apps/api/event';
import { validateScene, type Scene, type Character } from './scene';
export interface RunDetail { id:string; status:string; prompt:string; output:string; error:string|null; audit:string[]; project_id?:string|null }
export const officeApi = {
  async list(presets=false):Promise<Scene[]> { const scenes=await (isDesktop()?invoke<Scene[]>('office_list',{presets}):control<Scene[]>(presets?'/office-presets':'/offices'));return scenes.map(validateScene); },
  save(scene:Scene,presets=false):Promise<Scene> {validateScene(scene);return isDesktop()?invoke('office_save',{scene,presets}):control(`${presets?'/office-presets':'/offices'}/${encodeURIComponent(scene.id)}`,'PUT',scene);},
  remove(id:string,presets=false):Promise<unknown> {return isDesktop()?invoke('office_delete',{id,presets}):control(`${presets?'/office-presets':'/offices'}/${encodeURIComponent(id)}`,'DELETE');},
  activity():Promise<Character[]> {return isDesktop()?invoke('office_activity'):control('/offices/activity');},
  run(id:string):Promise<RunDetail|null> {return isDesktop()?invoke('office_run',{id}):control(`/offices/runs/${encodeURIComponent(id)}`);},
};
export async function officeEvents(refresh:()=>void,documents:()=>void,connection:(ok:boolean)=>void):Promise<()=>void> {
  if(isDesktop()) {const clean=await Promise.all([listen('office-activity-changed',refresh),listen('offices-changed',documents)]);connection(true);return ()=>clean.forEach(fn=>fn());}
  const source=new EventSource(`${CONTROL_BASE}/events`,{withCredentials:true});
  source.onopen=()=>{connection(true);refresh();documents();};source.onerror=()=>connection(false);
  for(const event of ['office_activity_changed','lagged']) source.addEventListener(event,refresh);
  source.addEventListener('offices_changed',documents);
  return ()=>source.close();
}
