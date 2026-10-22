import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { officeAgents } from './fixtures/office-agents.ts';
import { OfficeEditor } from '../src/lib/office/editor.ts';
import { presets, roomKey, validateScene, wizardScene, eventStatus, positions } from '../src/lib/office/scene.ts';
const clone = () => structuredClone(presets[0]);
test('all built-in scenes pass validation and retain their version', () => {
  assert.equal(presets.length,4);
  for (const scene of presets) assert.deepEqual(validateScene(scene),scene);
});
test('validation rejects malformed imports, references, overlaps and noninteger coordinates', () => {
  for(const mutate of [s=>s.version=2,s=>s.rooms[0].x=-1,s=>s.rooms[0].width=100,s=>s.furniture[0].x=NaN,s=>s.furniture[0].x=1.5,s=>s.desk_agents.missing='agent',s=>s.room_projects.missing='project',s=>s.rooms.push({...s.rooms[0],id:'overlap'}),s=>s.theme='external',s=>s.rooms[0].color='red',s=>s.extra=true,s=>s.furniture[0].kind='unknown',s=>s.desk_agents={'desk-0-0':'same','desk-0-1':'same'}]) {
    const scene=clone();mutate(scene);assert.throws(()=>validateScene(scene),/office\.(invalid_scene|room_overlap)/);
  }
  for(const value of [null,[],{},'scene'])assert.throws(()=>validateScene(value));
});
test('wizard output maps agents and projects into bounded, distinct rooms for each size', () => {
  for(const size of ['small','medium','large'])for(const theme of ['classic','pixel','night']) {
    const scene=wizardScene(size,['Research','Delivery'],{Research:{agents:['a','b'],project:'p'},Delivery:{agents:['c']}},theme,'Office');
    assert.equal(scene.theme,theme);assert.equal(scene.rooms.length,2);assert.equal(scene.room_projects['room-0'],'p');assert.deepEqual(Object.values(scene.desk_agents),['a','b','c']);assert.deepEqual(validateScene(scene),scene);
  }
  assert.throws(()=>wizardScene('small',[],{},'classic','Office'));
  assert.throws(()=>wizardScene('small',['Room'],{Room:{agents:Array.from({length:100},(_,i)=>String(i))}},'classic','Office'),/office.capacity/);
});
test('editor transactions snap, undo, redo, branch history and reject invalid edits atomically', () => {
  const editor=new OfficeEditor(clone());const start=structuredClone(editor.scene);
  editor.move('desk-0-0',{x:3.4,y:3.6,width:2,height:1});assert.equal(editor.scene.furniture[0].x,3);assert.equal(editor.scene.furniture[0].y,4);
  assert.deepEqual(editor.undo(),start);assert.equal(editor.canRedo,true);editor.redo();assert.equal(editor.scene.furniture[0].x,3);
  const before=structuredClone(editor.scene);assert.throws(()=>editor.move('desk-0-0',{x:999,y:0,width:2,height:1}));assert.deepEqual(editor.scene,before);
  editor.undo();editor.transact(s=>s.name='Renamed');assert.equal(editor.canRedo,false);assert.equal(editor.scene.name,'Renamed');
});
test('desk reassignment is unique and removing a desk or room removes its references', () => {
  const editor=new OfficeEditor(clone());editor.assign('desk-0-0','a');editor.assign('desk-0-1','a');assert.deepEqual(editor.scene.desk_agents,{'desk-0-1':'a'});
  editor.remove('desk-0-1');assert.deepEqual(editor.scene.desk_agents,{});editor.undo();assert.equal(editor.scene.desk_agents['desk-0-1'],'a');
  assert.throws(()=>editor.remove('room-0'));assert.equal(editor.scene.rooms.length,1);
});
test('event statuses and character positions stay stable with thirty agents', () => {
  for(const [raw,status] of Object.entries({running:'thinking',loading:'thinking',tool:'running_tool',running_tool:'running_tool',waiting_for_approval:'waiting_for_approval',awaiting_approval:'waiting_for_approval',paused:'sleeping',failed:'error',interrupted:'error',stopped:'sleeping',queued:'sleeping',completed:'idle'}))assert.equal(eventStatus(raw),status);
  const scene=clone();scene.desk_agents['desk-0-0']='taskflow-fixture-5';const actors=officeAgents();
  assert.ok(actors.every(c=>c.id===c.run_id&&c.agent));
  const result=positions(scene,actors);assert.equal(result.length,30);assert.equal(result[5].x,scene.furniture[0].x+1);assert.deepEqual(positions(scene,actors),result);assert.ok(result.every(c=>Number.isFinite(c.x)&&Number.isFinite(c.y)&&c.x>=0&&c.x<=16&&c.y>=0&&c.y<=12));
});
test('navigation is advanced, 3D is separately loaded and hidden frames are cancelled', () => {
  const read=p=>readFileSync(new URL(p,import.meta.url),'utf8');
  assert.match(read('../src/lib/uiLevel.ts'),/advancedTabs = \['office'/);
  assert.match(read('../src/lib/office/OfficeIso.svelte'),/import\('\.\/isometric'\)/);
  assert.match(read('../src/lib/office/isometric.ts'),/if\(!active\)\{cancelAnimationFrame\(frame\);return;/);
  // A deep $state proxy cannot be structuredClone'd by the editor, so "Discard Changes" would silently fail.
  assert.match(read('../src/lib/components/AgentOffice.svelte'),/pendingLoad=\$state\.raw</);
  // A rejected bounds edit (an overlap) puts the room's real value back in the field.
  assert.match(read('../src/lib/components/AgentOffice.svelte'),/bounds\(k,Number\(el\.value\)\);if\(item\)el\.value=String\(item\[k\]\)/);
});
test('every built-in room name has a translated label', async () => {
  const { default: { tr, en } } = await import('../src/lib/locales/office.ts');
  for (const scene of presets) for (const room of scene.rooms) {
    const key = roomKey(room.name);
    assert.ok(key in tr && key in en, key);
  }
});
test('dev office fixture reads its count and rotates statuses', async () => {
  const { officeFixtureCount, fixtureCharacters } = await import('../src/lib/dev/officeFixtures.ts');
  assert.equal(officeFixtureCount('?office_fixture=30', null), 30);
  assert.equal(officeFixtureCount('', '12'), 12);
  assert.equal(officeFixtureCount('?office_fixture=abc', null), 0);
  assert.equal(officeFixtureCount('?office_fixture=9999', null), 200);
  const a = fixtureCharacters(30, 0), b = fixtureCharacters(30, 1);
  assert.equal(a.length, 30); assert.notEqual(a[0].status, b[0].status);
  assert.equal(positions(clone(), a).length, 30);
});
test('overlapping rooms are reported as an overlap', () => {
  const scene = clone(); scene.rooms.push({ ...scene.rooms[0], id: 'room-x' });
  assert.throws(() => validateScene(scene), /office\.room_overlap/);
});
