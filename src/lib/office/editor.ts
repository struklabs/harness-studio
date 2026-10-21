import { validateScene, type Scene, type Bounds } from './scene.ts';
/** All edits are validated transactions. History stores snapshots, never live activity. */
export class OfficeEditor {
  private past: Scene[] = [];
  private future: Scene[] = [];
  scene: Scene;
  constructor(scene: Scene) { this.scene=validateScene(scene); }
  get canUndo() { return this.past.length>0; }
  get canRedo() { return this.future.length>0; }
  transact(change: (draft: Scene)=>void) {
    const draft=structuredClone(this.scene); change(draft); const next=validateScene(draft);
    if(JSON.stringify(next)===JSON.stringify(this.scene)) return this.scene;
    this.past.push(this.scene); if(this.past.length>100) this.past.shift(); this.future=[]; this.scene=next; return this.scene;
  }
  move(id:string,bounds:Bounds) { return this.transact(s=>{const item=[...s.rooms,...s.furniture].find(v=>v.id===id); if(item) Object.assign(item,Object.fromEntries(Object.entries(bounds).map(([k,v])=>[k,Math.round(v)])));}); }
  assign(desk:string,agent:string) {return this.transact(s=>{for(const [key,value] of Object.entries(s.desk_agents)) if(value===agent || key===desk) delete s.desk_agents[key]; if(agent) s.desk_agents[desk]=agent;});}
  remove(id:string) {return this.transact(s=>{s.rooms=s.rooms.filter(v=>v.id!==id);s.furniture=s.furniture.filter(v=>v.id!==id);delete s.desk_agents[id];delete s.room_projects[id];});}
  undo() {const scene=this.past.pop();if(scene){this.future.push(this.scene);this.scene=scene;}return this.scene;}
  redo() {const scene=this.future.pop();if(scene){this.past.push(this.scene);this.scene=scene;}return this.scene;}
}
