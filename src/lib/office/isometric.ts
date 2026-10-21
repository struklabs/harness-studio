import * as THREE from 'three';
import { dimensions, positions, statusColor, type Scene, type Character } from './scene';
export interface IsoRenderer { update:(scene:Scene,characters:Character[],active:boolean)=>void;dispose:()=>void }
/** Render on scene changes and resize only; no idle animation loop or hidden frames. */
export function mountIsometric(host:HTMLElement,onopen:(c:Character)=>void):IsoRenderer {
  const renderer=new THREE.WebGLRenderer({antialias:true,alpha:false});renderer.setPixelRatio(Math.min(devicePixelRatio,2));renderer.setClearColor('#e6e9e5');host.appendChild(renderer.domElement);
  const world=new THREE.Scene(); const camera=new THREE.OrthographicCamera(-20,20,15,-15,0.1,200);
  let active=true, disposed=false, frame=0, meshes:THREE.Mesh[]=[];
  function clear(){for(const mesh of meshes){world.remove(mesh);mesh.geometry.dispose();(mesh.material as THREE.Material).dispose();}meshes=[];}
  function box(x:number,z:number,w:number,d:number,h:number,color:string,y=0){const mesh=new THREE.Mesh(new THREE.BoxGeometry(w,h,d),new THREE.MeshLambertMaterial({color}));mesh.position.set(x,y+h/2,z);world.add(mesh);meshes.push(mesh);return mesh;}
  world.add(new THREE.AmbientLight(0xffffff,2));const sun=new THREE.DirectionalLight(0xffffff,3);sun.position.set(10,25,10);world.add(sun);
  function draw(){if(disposed||!active)return;cancelAnimationFrame(frame);frame=requestAnimationFrame(()=>{if(active&&!disposed)renderer.render(world,camera);});}
  function resize(){if(disposed)return;const w=host.clientWidth,h=host.clientHeight;renderer.setSize(w,h,false);const ratio=w/Math.max(1,h);camera.left=-20*ratio;camera.right=20*ratio;camera.top=20;camera.bottom=-20;camera.updateProjectionMatrix();draw();}
  const observer=new ResizeObserver(resize);observer.observe(host);
  const ray=new THREE.Raycaster();
  function click(e:MouseEvent){const r=renderer.domElement.getBoundingClientRect();ray.setFromCamera(new THREE.Vector2((e.clientX-r.left)/r.width*2-1,-(e.clientY-r.top)/r.height*2+1),camera);const hit=ray.intersectObjects(meshes).find(v=>v.object.userData.character);if(hit)onopen(hit.object.userData.character);}
  renderer.domElement.addEventListener('click',click);
  return {update(scene,characters,visible){active=visible;if(!active){cancelAnimationFrame(frame);return;}clear();const [w,h]=dimensions[scene.size];camera.position.set(w+16,30,h+16);camera.lookAt(w/2,0,h/2);box(w/2,h/2,w,h,0.15,scene.theme==='night'?'#29384a':'#d8d8c9');
    for(const r of scene.rooms){box(r.x+r.width/2,r.y+r.height/2,r.width,r.height,.12,r.color,.15);box(r.x+r.width/2,r.y,r.width,.12,1.5,r.color);box(r.x,r.y+r.height/2,.12,r.height,1.5,r.color);}
    for(const f of scene.furniture){box(f.x+f.width/2,f.y+f.height/2,f.width*.9,f.height*.9,f.kind==='plant'?1.4:f.kind==='board'?1.6:.75,f.kind==='plant'?'#578d65':f.kind==='board'?'#d9e7dc':'#b69a7a',.3);if(f.kind==='desk')box(f.x+f.width/2,f.y+.35,.65,.15,.5,'#344958',1.05);}
    for(const c of positions(scene,characters)){const body=box(c.x,c.y,.55,.45,.8,c.kind==='device'?'#72949b':'#638ba9',.3);body.userData.character=c;const head=box(c.x,c.y,.4,.4,.4,'#ddbea0',1.1);head.userData.character=c;const badge=box(c.x+.3,c.y,.2,.2,.2,statusColor[c.status],1.5);badge.userData.character=c;}
    resize();},dispose(){disposed=true;cancelAnimationFrame(frame);observer.disconnect();renderer.domElement.removeEventListener('click',click);clear();renderer.dispose();renderer.forceContextLoss();renderer.domElement.remove();}};
}
