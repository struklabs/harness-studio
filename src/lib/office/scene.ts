import bundled from '../../../shared/office-presets.json' with { type: 'json' };
export type OfficeSize = 'small' | 'medium' | 'large';
export type OfficeTheme = 'classic' | 'pixel' | 'night';
export type OfficeStatus = 'idle' | 'thinking' | 'running_tool' | 'waiting_for_approval' | 'error' | 'sleeping';
export interface Bounds { x: number; y: number; width: number; height: number }
export interface Room extends Bounds { id: string; name: string; color: string }
export interface Furniture extends Bounds { id: string; kind: 'desk' | 'meeting' | 'kitchen' | 'plant' | 'board' }
export interface Scene { version: number; id: string; name: string; size: OfficeSize; theme: OfficeTheme; rooms: Room[]; furniture: Furniture[]; desk_agents: Record<string,string>; room_projects: Record<string,string> }
export interface Character { id: string; name: string; kind: string; status: OfficeStatus; run_id?: string | null; project_id?: string | null; agent?: string | null }
export const dimensions: Record<OfficeSize, [number,number]> = { small:[16,12], medium:[24,16], large:[32,20] };
export const presets = bundled as Scene[];
const text = (s: unknown): s is string => typeof s === 'string' && !!s.trim() && new TextEncoder().encode(s).length <= 200 && !/[\u0000-\u001f\u007f-\u009f]/.test(s);
const id = (s: unknown): s is string => typeof s === 'string' && /^[a-zA-Z0-9_-]{1,100}$/.test(s);
const record = (v: unknown): v is Record<string,unknown> => !!v && typeof v === 'object' && !Array.isArray(v);
function fields(v: Record<string,unknown>, keys: string[]) { return Object.keys(v).every(k => keys.includes(k)) && keys.every(k => Object.hasOwn(v,k)); }
export function validateScene(value: unknown): Scene {
  const fail = (): never => { throw new Error('office.invalid_scene'); };
  if (!record(value) || !fields(value,['version','id','name','size','theme','rooms','furniture','desk_agents','room_projects'])) return fail();
  const s = value as unknown as Scene;
  if (s.version !== 1 || !id(s.id) || !text(s.name) || !Object.hasOwn(dimensions,s.size) || !['classic','pixel','night'].includes(s.theme) || !Array.isArray(s.rooms) || !s.rooms.length || s.rooms.length > 32 || !Array.isArray(s.furniture) || s.furniture.length > 256 || !record(s.desk_agents) || !record(s.room_projects) || Object.keys(s.desk_agents).length > 256 || Object.keys(s.room_projects).length > 32) return fail();
  const [w,h] = dimensions[s.size];
  const bounds = (b: Bounds) => [b.x,b.y,b.width,b.height].every(Number.isSafeInteger) && b.x >= 0 && b.y >= 0 && b.width > 0 && b.height > 0 && b.x+b.width <= w && b.y+b.height <= h;
  const ids = new Set<string>();
  for (const r of s.rooms) {
    if (!record(r) || !fields(r,['id','name','color','x','y','width','height']) || !id(r.id) || ids.has(r.id) || !text(r.name) || !/^#[a-fA-F0-9]{6}$/.test(r.color) || !bounds(r)) return fail();
    ids.add(r.id);
  }
  for (let i=0;i<s.rooms.length;i++) for (const b of s.rooms.slice(i+1)) { const a=s.rooms[i]; if (a.x < b.x+b.width && b.x < a.x+a.width && a.y < b.y+b.height && b.y < a.y+a.height) throw new Error('office.room_overlap'); }
  for (const f of s.furniture) {
    if (!record(f) || !fields(f,['id','kind','x','y','width','height']) || !id(f.id) || ids.has(f.id) || !['desk','meeting','kitchen','plant','board'].includes(f.kind) || !bounds(f)) return fail();
    ids.add(f.id);
  }
  const assigned = new Set<string>();
  for (const [desk,agent] of Object.entries(s.desk_agents)) {
    if (!text(agent) || assigned.has(agent) || !s.furniture.some(f => f.id===desk && f.kind==='desk')) return fail(); assigned.add(agent);
  }
  for (const [room,project] of Object.entries(s.room_projects)) if (!text(project) || !s.rooms.some(r => r.id===room)) return fail();
  return structuredClone(s);
}
export function newScene(preset: Scene, name = preset.name): Scene {
  return validateScene({ ...structuredClone(preset), id: crypto.randomUUID(), name });
}
export function wizardScene(size: OfficeSize, departments: string[], assignments: Record<string,{agents:string[];project?:string}>, theme: OfficeTheme, name: string): Scene {
  if (!departments.length || departments.length > 8) throw new Error('office.invalid_scene');
  const [w,h] = dimensions[size]; const cols = departments.length>1 ? 2 : 1; const rows = Math.ceil(departments.length/cols);
  const scene: Scene = {version:1,id:crypto.randomUUID(),name,size,theme,rooms:[],furniture:[],desk_agents:{},room_projects:{}};
  departments.forEach((name,i) => {
    const x=1+(i%cols)*Math.floor(w/cols), y=1+Math.floor(i/cols)*Math.floor(h/rows), width=Math.floor(w/cols)-2, height=Math.floor(h/rows)-2;
    const room = {id:`room-${i}`,name,color:['#88b9b0','#a6a0d0','#d2ac7e','#88acc9'][i%4],x,y,width,height}; scene.rooms.push(room);
    const a=assignments[name]; if(a?.project) scene.room_projects[room.id]=a.project;
    let j=0;
    for(let dy=1;dy<height-1;dy+=2) for(let dx=1;dx<width-2;dx+=3) {
      const desk={id:`desk-${i}-${j}`,kind:'desk' as const,x:x+dx,y:y+dy,width:2,height:1}; scene.furniture.push(desk);
      if(a?.agents[j]) scene.desk_agents[desk.id]=a.agents[j]; j++;
    }
    if ((a?.agents.length ?? 0)>j) throw new Error('office.capacity');
  });
  return validateScene(scene);
}
export function eventStatus(raw: string): OfficeStatus {
  if (['running','loading','thinking'].includes(raw)) return 'thinking';
  if (['tool','running_tool'].includes(raw)) return 'running_tool';
  if (['waiting_for_approval','awaiting_approval'].includes(raw)) return 'waiting_for_approval';
  if (['failed','error','interrupted'].includes(raw)) return 'error';
  if (['queued','stopped','sleeping','cancelling','paused'].includes(raw)) return 'sleeping';
  return 'idle';
}
export const statusColor: Record<OfficeStatus,string> = {idle:'#518578',thinking:'#557cda',running_tool:'#bd8335',waiting_for_approval:'#af6bb8',error:'#d65665',sleeping:'#83919f'};
export function positions(scene: Scene, characters: Character[]) {
  const [w,h]=dimensions[scene.size]; const desks=scene.furniture.filter(f=>f.kind==='desk');
  const reserved=new Set(Object.keys(scene.desk_agents)); let free=0,standing=0;
  return characters.map(character => {
    const assigned=Object.entries(scene.desk_agents).find(([,a])=>a===character.id)?.[0];
    const room=scene.rooms.find(r=>scene.room_projects[r.id]===character.project_id);
    const desk=desks.find(d=>d.id===assigned) ?? (character.kind==='agent' ? desks.filter(d=>!reserved.has(d.id) && (!room || d.x>=room.x && d.x<room.x+room.width && d.y>=room.y && d.y<room.y+room.height))[free++] : undefined);
    const index=standing++;
    return {...character,x:desk?desk.x+desk.width/2:0.8+(index%Math.floor(w-1)),y:desk?Math.min(h-0.5,desk.y+1.45):h-0.65-Math.floor(index/Math.floor(w-1))*0.8};
  });
}
/** Locale key of a built-in room name ("Meeting Room" -> `office.room_meeting_room`). */
export function roomKey(name: string): string {
  return `office.room_${name.toLowerCase().replace(/\s+/g, '_')}`;
}
